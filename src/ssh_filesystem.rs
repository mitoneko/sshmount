mod bi_hash_map;
mod file_handle;
mod inode;
mod mutex_lock_ext;

use file_handle::{Dhandles, Fhandles};
use inode::Inodes;
use mutex_lock_ext::MutexLockExt;

use anyhow::Context;
use fuser::{
    BsdFileFlags, Errno, FileAttr, FileHandle, Filesystem, FopenFlags, Generation, INodeNo,
    LockOwner, OpenFlags as OpenFlagsFuser, RenameFlags, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEntry, Request, WriteFlags,
};
use log::{debug, error, warn};
use ssh2::{ErrorCode, OpenFlags as OpenFlagsSsh2, OpenType, Session, Sftp};
use std::{
    ffi::OsStr,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// FUSE ファイルシステム実装
pub struct Sshfs {
    _session: Session,
    sftp: Sftp,
    inodes: Inodes,
    fhandls: Fhandles,
    dhandls: Dhandles, // opendir時点のディレクトリ一覧を保持する。
    top_path: PathBuf,
}

impl Sshfs {
    pub fn new<P: AsRef<Path>>(session: Session, path: P) -> anyhow::Result<Self> {
        let inodes = Inodes::new();
        let top_path: PathBuf = path.as_ref().into();
        inodes.add(&top_path);
        let sftp = session
            .sftp()
            .inspect_err(|_| {
                error!("Failed to create sftp from session.");
            })
            .context("Failed to create sftp from session.(Sshfs::new)")?;
        debug!(
            "[Sshfs::new] connect path: <{:?}>, inodes=<{:?}>",
            top_path, inodes
        );
        Ok(Self {
            _session: session,
            sftp,
            inodes,
            fhandls: Fhandles::new(),
            dhandls: Dhandles::new(),
            top_path: top_path,
        })
    }

    /// ssh2経由でファイルのステータスを取得する。
    /// 副作用:取得に成功した場合、inodesにパスを登録する。
    fn getattr_from_ssh2(&self, path: &Path, uid: u32, gid: u32) -> Result<FileAttr, Error> {
        let attr_ssh2 = self.sftp.lstat(path)?;
        let kind = Self::conv_file_kind_ssh2fuser(&attr_ssh2.file_type())?;
        let ino = self.inodes.add(path);
        Ok(FileAttr {
            ino,
            size: attr_ssh2.size.unwrap_or(0),
            blocks: attr_ssh2.size.unwrap_or(0) / 512 + 1,
            atime: UNIX_EPOCH + Duration::from_secs(attr_ssh2.atime.unwrap_or(0)),
            mtime: UNIX_EPOCH + Duration::from_secs(attr_ssh2.mtime.unwrap_or(0)),
            ctime: UNIX_EPOCH + Duration::from_secs(attr_ssh2.mtime.unwrap_or(0)),
            crtime: UNIX_EPOCH,
            kind,
            perm: attr_ssh2.perm.unwrap_or(0o666) as u16,
            nlink: 1,
            uid,
            gid,
            rdev: 0,
            blksize: 512,
            flags: 0,
        })
    }

    fn conv_file_kind_ssh2fuser(filetype: &ssh2::FileType) -> Result<fuser::FileType, Error> {
        match filetype {
            ssh2::FileType::NamedPipe => Ok(fuser::FileType::NamedPipe),
            ssh2::FileType::CharDevice => Ok(fuser::FileType::CharDevice),
            ssh2::FileType::BlockDevice => Ok(fuser::FileType::BlockDevice),
            ssh2::FileType::Directory => Ok(fuser::FileType::Directory),
            ssh2::FileType::RegularFile => Ok(fuser::FileType::RegularFile),
            ssh2::FileType::Symlink => Ok(fuser::FileType::Symlink),
            ssh2::FileType::Socket => Ok(fuser::FileType::Socket),
            ssh2::FileType::Other(_) => Err(Error(Errno::EBADF)),
        }
    }

    /// rmdir失敗時のssh2エラーをerrnoに変換する。
    /// 一部のSFTPサーバ/ライブラリでは、空でないディレクトリ削除時に
    /// ENOTEMPTY相当を直接返さず、曖昧な失敗コードになるため補正する。
    fn rmdir_errno(e: ssh2::Error) -> Errno {
        debug!("[rmdir_errno] ssh2::Error => {:?}", e);
        match e.code() {
            ErrorCode::Session(-31) | ErrorCode::SFTP(4) => Errno::ENOTEMPTY,
            _ => Error::from(e).into(),
        }
    }

    fn conv_timeornow2systemtime(time: &fuser::TimeOrNow) -> SystemTime {
        match time {
            fuser::TimeOrNow::SpecificTime(t) => *t,
            fuser::TimeOrNow::Now => SystemTime::now(),
        }
    }
}

impl Filesystem for Sshfs {
    fn lookup(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        debug!("[lookup] start: parent={}, name={:?}", parent, name);
        let Some(mut path) = self.inodes.get_path(parent) else {
            error!("[lookup] 親ディレクトリの検索に失敗 inode={}", parent);
            reply.error(Errno::ENOENT);
            return;
        };
        path.push(Path::new(name));
        match self.getattr_from_ssh2(&path, req.uid(), req.gid()) {
            Ok(attr) => {
                debug!("[lookup Ok] inode:{:?}, kind:{:?}", attr.ino, attr.kind);
                reply.entry(&Duration::from_secs(1), &attr, Generation(0))
            }
            Err(e) => {
                warn!("[lookup err] {}", e);
                reply.error(e.into())
            }
        };
    }

    fn getattr(&self, req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        debug!("[getattr] start: ino={}", ino);
        let Some(path) = self.inodes.get_path(ino) else {
            error!("[getattr] path取得失敗: inode={}", ino);
            reply.error(Errno::ENOENT);
            return;
        };
        match self.getattr_from_ssh2(&path, req.uid(), req.gid()) {
            Ok(attr) => {
                debug!("[getattr Ok] inode:{:?}, kind:{:?}", attr.ino, attr.kind);
                reply.attr(&Duration::from_secs(1), &attr);
            }
            Err(e) => {
                warn!("[getattr err] {}", e);
                reply.error(e.into())
            }
        };
    }

    /// ディレクトリを開く。この時点の一覧を取得し、ハンドルに保持する。
    /// (修正)従来はreaddirのたびに一覧を取り直していたため、読み出し途中の
    /// unlinkでoffsetがずれ、remove_dir_allなどでエントリが欠落していた。
    fn opendir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _flags: OpenFlagsFuser,
        reply: fuser::ReplyOpen,
    ) {
        debug!("[opendir] start: ino={}", ino);
        let Some(path) = self.inodes.get_path(ino) else {
            reply.error(Errno::ENOENT);
            return;
        };
        match self.sftp.readdir(&path) {
            Ok(mut dir) => {
                let cur_file_attr = ssh2::FileStat {
                    size: None,
                    uid: None,
                    gid: None,
                    perm: Some(libc::S_IFDIR),
                    atime: None,
                    mtime: None,
                }; // "." ".."の解決用。 attr ディレクトリであることのみを示す。
                dir.insert(0, (Path::new("..").into(), cur_file_attr.clone()));
                dir.insert(0, (Path::new(".").into(), cur_file_attr));
                let item_count = dir.len();
                let fh = FileHandle(self.dhandls.add_dir(dir));
                debug!("[opendir ok] fh:{}, count:{}", fh, item_count);
                reply.opened(fh, FopenFlags::empty());
            }
            Err(e) => {
                warn!("[opendir]ssh2::readdir内でエラー発生-- {:?}", e);
                reply.error(Error::from(e).into());
            }
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        debug!("[readdir] start: ino={}, offset={}", ino, offset);
        // (修正)opendirで保持したスナップショットを使う。
        let Some(dir) = self.dhandls.get_dir(fh.0) else {
            reply.error(Errno::EBADF);
            return;
        };
        for (i, f) in (offset + 1..).zip(dir.iter().skip(offset as usize)) {
            let entry_ino = if f.0 == Path::new(".") {
                ino
            } else if f.0 == Path::new("..") {
                let Some(cur_path) = self.inodes.get_path(ino) else {
                    warn!("[readdir] 現ディレクトリの検索に失敗 inode={}", ino);
                    reply.error(Errno::ENOENT);
                    return;
                };
                if cur_path != self.top_path {
                    let mut parrent_path = cur_path;
                    parrent_path.pop();
                    let Some(parrent_ino) = self.inodes.get_inode(&parrent_path) else {
                        warn!(
                            "[readdir] 親ディレクトリの検索に失敗 path={:?}",
                            parrent_path
                        );
                        reply.error(Errno::ENOENT);
                        return;
                    };
                    parrent_ino
                } else {
                    INodeNo(1) // ルートディレクトリの親は自分自身にする。
                }
            } else {
                self.inodes.add(&f.0)
            };
            let name = match f.0.file_name() {
                Some(n) => n,
                None => f.0.as_os_str(),
            };
            let filetype = &f.1.file_type();
            let filetype = match Self::conv_file_kind_ssh2fuser(filetype) {
                Ok(t) => t,
                Err(e) => {
                    warn!(
                        "[readdir]ファイルタイプ解析失敗: inode={}, name={:?}",
                        entry_ino, name
                    );
                    reply.error(e.into());
                    return;
                }
            };
            if reply.add(entry_ino, i, filetype, name) {
                break;
            }
        }
        debug!("[readdir] Ok: ino={}, offset={}", ino, offset);
        reply.ok();
    }

    /// ディレクトリを閉じ、保持していた一覧を破棄する。
    fn releasedir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlagsFuser,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("[releasedir] start: fh={}", fh);
        self.dhandls.del_dir(fh.0);
        reply.ok();
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        debug!("[readlink] start: ino={}", ino);
        let Some(path) = self.inodes.get_path(ino) else {
            error!("[readlink] 親ディレクトリの検索に失敗 {ino}");
            reply.error(Errno::ENOENT);
            return;
        };
        match self.sftp.readlink(&path) {
            Ok(p) => {
                debug!("[readlink] ok: ret_path => {:?}", p);
                reply.data(p.as_os_str().to_string_lossy().as_bytes());
            }
            Err(e) => {
                let e = Error::from(e);
                warn!("[readlink] ssh2::readlink error => {e}");
                reply.error(e.into());
            }
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlagsFuser, reply: fuser::ReplyOpen) {
        debug!("[open] start: ino={}, flags={:x}", ino, flags);
        let Some(file_name) = self.inodes.get_path(ino) else {
            reply.error(Errno::ENOENT);
            return;
        };

        let mut flags_ssh2 = OpenFlagsSsh2::empty();
        if flags.0 & libc::O_WRONLY != 0 {
            flags_ssh2.insert(OpenFlagsSsh2::WRITE);
        } else if flags.0 & libc::O_RDWR != 0 {
            flags_ssh2.insert(OpenFlagsSsh2::READ);
            flags_ssh2.insert(OpenFlagsSsh2::WRITE);
        } else {
            flags_ssh2.insert(OpenFlagsSsh2::READ);
        }
        if flags.0 & libc::O_APPEND != 0 {
            flags_ssh2.insert(OpenFlagsSsh2::APPEND);
        }
        if flags.0 & libc::O_CREAT != 0 {
            flags_ssh2.insert(OpenFlagsSsh2::CREATE);
        }
        if flags.0 & libc::O_TRUNC != 0 {
            flags_ssh2.insert(OpenFlagsSsh2::TRUNCATE);
        }
        if flags.0 & libc::O_EXCL != 0 {
            flags_ssh2.insert(OpenFlagsSsh2::EXCLUSIVE);
        }

        debug!(
            "[open] filename='{:?}', openflag = {:?}, bit = {:x}",
            file_name,
            flags_ssh2,
            flags_ssh2.bits()
        );
        match self
            .sftp
            .open_mode(&file_name, flags_ssh2, 0o777, ssh2::OpenType::File)
        {
            Ok(file) => {
                let fh = FileHandle(self.fhandls.add_file(file));
                reply.opened(fh, FopenFlags::empty());
            }
            Err(e) => {
                let e = Error::from(e);
                warn!(
                    "file-open error: filename='{:?}', mode={:?}, err={}",
                    file_name, flags_ssh2, e
                );
                reply.error(e.into());
            }
        }
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlagsFuser,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("[release] start: fh={}", fh);
        self.fhandls.del_file(fh.0);
        reply.ok();
    }

    fn read(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlagsFuser,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        debug!("[read] start: fh={}, offset={}, size={}", fh, offset, size);
        let Some(file_mutex) = self.fhandls.get_file(fh.0) else {
            let err = Error(Errno::EBADF);
            warn!("[read] file handle not found. err={}", err);
            reply.error(err.into());
            return;
        };
        let mut file = file_mutex.lock_or_panic("read", "file handle");

        if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)) {
            let err = Error::from(e);
            warn!("[read] file seek error. err={}", err);
            reply.error(err.into());
            return;
        }
        let mut buff = vec![0; size as usize];
        let mut read_size: usize = 0;
        while read_size < size as usize {
            match file.read(&mut buff[read_size..]) {
                Ok(s) => {
                    if s == 0 {
                        break;
                    };
                    read_size += s;
                }
                Err(e) => {
                    reply.error(Error::from(e).into());
                    return;
                }
            }
        }
        buff.resize(read_size, 0u8);
        reply.data(&buff);
        debug!("[read] Ok: read_size={}", read_size);
    }

    fn write(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlagsFuser,
        _lock_owner: Option<LockOwner>,
        reply: fuser::ReplyWrite,
    ) {
        debug!(
            "[write] start: fh={}, offset={}, size={}",
            fh,
            offset,
            data.len()
        );
        let Some(file_mutex) = self.fhandls.get_file(fh.0) else {
            let err = Error(Errno::EBADF);
            warn!("[write] file handle not found. err={}", err);
            reply.error(err.into());
            return;
        };
        let file = &mut file_mutex.lock_or_panic("write", "file handle");

        if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)) {
            let err = Error::from(e);
            warn!("[write] file seek error. err={}", err);
            reply.error(err.into());
            return;
        }
        let mut buf = data;
        while !buf.is_empty() {
            let cnt = match file.write(buf) {
                Ok(cnt) => cnt,
                Err(e) => {
                    let err = Error::from(e);
                    warn!("[write] file write error. err={}", err);
                    reply.error(err.into());
                    return;
                }
            };
            buf = &buf[cnt..];
        }
        reply.written(data.len() as u32);
        debug!("[write] Ok: written_size={}", data.len());
    }

    fn lseek(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: i64,
        whence: i32,
        reply: fuser::ReplyLseek,
    ) {
        debug!(
            "[lseek] start: fh={}, offset={}, whence={}",
            fh, offset, whence
        );
        let seek_from = match whence {
            libc::SEEK_SET => {
                if offset >= 0 {
                    SeekFrom::Start(offset as u64)
                } else {
                    let err = Error(Errno::EINVAL);
                    warn!("[lseek] invalid offset for SEEK_SET: offset={}", offset);
                    reply.error(err.into());
                    return;
                }
            }
            libc::SEEK_CUR => SeekFrom::Current(offset),
            libc::SEEK_END => SeekFrom::End(offset),
            _ => {
                let err = Error(Errno::EINVAL);
                warn!("[lseek] invalid whence value: whence={}", whence);
                reply.error(err.into());
                return;
            }
        };
        let Some(file_mutex) = self.fhandls.get_file(fh.0) else {
            let err = Error(Errno::EBADF);
            warn!("[lseek] file handle not found. err={}", err);
            reply.error(err.into());
            return;
        };
        let file = &mut file_mutex.lock_or_panic("lseek", "file handle");
        let pos = match file.seek(seek_from) {
            Ok(p) => p,
            Err(e) => {
                let err = Error::from(e);
                warn!("[lseek] file seek error. err={}", err);
                reply.error(err.into());
                return;
            }
        };
        reply.offset(pos as i64);
        debug!("[lseek] Ok: new_offset={}", pos);
    }

    fn mknod(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        debug!(
            "[mknod] start: parent={}, name={:?}, mode={:o}, umask={:o}",
            parent, name, mode, umask
        );
        if mode & libc::S_IFMT != libc::S_IFREG {
            let err = Error(Errno::EPERM);
            warn!("[mknod] only regular files are supported. mode={:o}", mode);
            reply.error(err.into());
            return;
        }
        let mode = mode & (!umask | libc::S_IFMT);
        let Some(mut new_name) = self.inodes.get_path(parent) else {
            let err = Error(Errno::ENOENT);
            warn!(
                "[mknod] parent directory not found. parent inode={}",
                parent
            );
            reply.error(err.into());
            return;
        };
        new_name.push(name);
        if let Err(e) = self.sftp.open_mode(
            &new_name,
            OpenFlagsSsh2::CREATE,
            mode as i32,
            OpenType::File,
        ) {
            let err = Error::from(e);
            warn!(
                "[mknod] failed to create file. path={:?}, mode={:o}, err={}",
                new_name, mode, err
            );
            reply.error(err.into());
            return;
        }
        let new_attr = match self.getattr_from_ssh2(&new_name, req.uid(), req.gid()) {
            Ok(a) => a,
            Err(e) => {
                warn!(
                    "[mknod] failed to get attributes for new file. path={:?}, err={}",
                    new_name, e
                );
                reply.error(e.into());
                return;
            }
        };
        reply.entry(&Duration::from_secs(1), &new_attr, Generation(0));
        debug!(
            "[mknod] Ok: created file {:?} with inode {}",
            new_name, new_attr.ino
        );
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: fuser::ReplyEmpty) {
        debug!("[unlink] start: parent={}, name={:?}", parent, name);
        let Some(mut path) = self.inodes.get_path(parent) else {
            let err = Error(Errno::ENOENT);
            warn!(
                "[unlink] parent directory not found. parent inode={}",
                parent
            );
            reply.error(err.into());
            return;
        };
        path.push(name);
        match self.sftp.unlink(&path) {
            Ok(_) => {
                self.inodes.del_inode_with_path(&path);
                reply.ok();
                debug!("[unlink] Ok: removed file {:?}", path);
            }
            Err(e) => {
                let err = Error::from(e);
                warn!("[unlink] failed to remove file {:?}. err={}", path, err);
                reply.error(err.into());
            }
        }
    }

    fn mkdir(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        debug!(
            "[mkdir] start: parent={}, name={:?}, mode={:o}, umask={:o}",
            parent, name, mode, umask
        );
        let Some(mut path) = self.inodes.get_path(parent) else {
            let err = Error(Errno::ENOENT);
            warn!(
                "[mkdir] parent directory not found. parent inode={}",
                parent
            );
            reply.error(err.into());
            return;
        };
        path.push(name);

        let mode = (mode & (!umask) & 0o777) as i32;

        match self.sftp.mkdir(&path, mode) {
            Ok(_) => match self.getattr_from_ssh2(&path, req.uid(), req.gid()) {
                Ok(attr) => {
                    reply.entry(&Duration::from_secs(1), &attr, Generation(0));
                    debug!(
                        "[mkdir] Ok: created directory {:?} with inode {}",
                        path, attr.ino
                    );
                }
                Err(e) => {
                    warn!(
                        "[mkdir] failed to get attributes for new directory {:?}. err={}",
                        path, e
                    );
                    reply.error(e.into());
                }
            },
            Err(e) => {
                let err = Error::from(e);
                warn!("[mkdir] failed to create directory {:?}. err={}", path, err);
                reply.error(err.into());
            }
        }
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: fuser::ReplyEmpty) {
        debug!("[rmdir] start: parent={}, name={:?}", parent, name);
        let Some(mut path) = self.inodes.get_path(parent) else {
            let err = Error(Errno::ENOENT);
            warn!(
                "[rmdir] parent directory not found. parent inode={}",
                parent
            );
            reply.error(err.into());
            return;
        };
        path.push(name);
        match self.sftp.rmdir(&path) {
            Ok(_) => {
                self.inodes.del_inode_with_path(&path);
                reply.ok();
                debug!("[rmdir] Ok: removed directory {:?}", path);
            }
            Err(e) => {
                let err = Error(Self::rmdir_errno(e));
                warn!("[rmdir] failed to remove directory {:?}. err={}", path, err);
                reply.error(err.into());
            }
        }
    }

    fn symlink(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        link: &Path,
        reply: ReplyEntry,
    ) {
        debug!(
            "[symlink] start: parent={}, name={:?}, link={:?}",
            parent, name, link
        );
        let Some(mut target) = self.inodes.get_path(parent) else {
            let err = Error(Errno::ENOENT);
            warn!(
                "[symlink] parent directory not found. parent inode={}",
                parent
            );
            reply.error(err.into());
            return;
        };
        target.push(name);
        match self.sftp.symlink(link, &target) {
            Ok(_) => match self.getattr_from_ssh2(&target, req.uid(), req.gid()) {
                Ok(attr) => {
                    reply.entry(&Duration::from_secs(1), &attr, Generation(0));
                    debug!(
                        "[symlink] Ok: created symlink {:?} -> {:?} with inode {}",
                        target, link, attr.ino
                    );
                }
                Err(e) => {
                    warn!(
                        "[symlink] failed to get attributes for new symlink {:?}. err={}",
                        target, e
                    );
                    reply.error(e.into());
                }
            },
            Err(e) => {
                let err = Error::from(e);
                warn!(
                    "[symlink] failed to create symlink {:?} -> {:?}. err={}",
                    target, link, err
                );
                reply.error(err.into());
            }
        }
    }

    fn setattr(
        &self,
        req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        atime: Option<fuser::TimeOrNow>,
        mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        debug!(
            "[setattr] start: ino={}, mode={:?}, size={:?}, atime={:?}, mtime={:?}",
            ino, mode, size, atime, mtime
        );
        let stat = ssh2::FileStat {
            size,
            uid: None,
            gid: None,
            perm: mode,
            atime: atime.map(|t| {
                Self::conv_timeornow2systemtime(&t)
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }),
            mtime: mtime.map(|t| {
                Self::conv_timeornow2systemtime(&t)
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }),
        };
        let Some(filename) = self.inodes.get_path(ino) else {
            let err = Error(Errno::ENOENT);
            warn!("[setattr] file not found. ino={}", ino);
            reply.error(err.into());
            return;
        };
        match self.sftp.setstat(&filename, stat) {
            Ok(_) => {
                let stat = self.getattr_from_ssh2(&filename, req.uid(), req.gid());
                match stat {
                    Ok(s) => {
                        reply.attr(&Duration::from_secs(1), &s);
                        debug!("[setattr] Ok: updated attributes for {:?}", filename);
                    }
                    Err(e) => {
                        warn!(
                            "[setattr] failed to get updated attributes for {:?}. err={}",
                            filename, e
                        );
                        reply.error(e.into());
                    }
                }
            }
            Err(e) => {
                let err = Error::from(e);
                warn!(
                    "[setattr] failed to set attributes for {:?}. err={}",
                    filename, err
                );
                reply.error(err.into());
            }
        }
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: fuser::ReplyEmpty,
    ) {
        debug!(
            "[rename] start: parent={}, name={:?}, newparent={}, newname={:?}, flags={}",
            parent, name, newparent, newname, flags
        );
        let Some(mut old_path) = self.inodes.get_path(parent) else {
            let err = Error(Errno::ENOENT);
            warn!(
                "[rename] old parent directory not found. parent inode={}",
                parent
            );
            reply.error(err.into());
            return;
        };
        old_path.push(name);

        let Some(mut new_path) = self.inodes.get_path(newparent) else {
            let err = Error(Errno::ENOENT);
            warn!(
                "[rename] new parent directory not found. newparent inode={}",
                newparent
            );
            reply.error(err.into());
            return;
        };
        new_path.push(newname);

        // (修正)RENAME_EXCHANGE(入れ替え)はSFTPでは実現できない。
        // 従来はATOMICフラグに誤って対応させていた。
        if flags.contains(RenameFlags::RENAME_EXCHANGE) {
            let err = Error(Errno::ENOTSUP);
            warn!("[rename] RENAME_EXCHANGE is not supported. flags={}", flags);
            reply.error(err.into());
            return;
        }

        // (修正)従来はRENAME_NOREPLACE指定時にだけ移動先を削除しており、判定が逆だった。
        // 通常のrename(flags=0)はPOSIXでは移動先を上書きするが、SFTPのrenameは
        // 移動先が存在すると失敗する(EIOになる)。そのため、移動先を手動で消してから行う。
        // RENAME_NOREPLACE指定時は、移動先が存在すればEEXISTを返す。
        if let Ok(new_stat) = self.sftp.lstat(&new_path) {
            if flags.contains(RenameFlags::RENAME_NOREPLACE) {
                let err = Error(Errno::EEXIST);
                warn!(
                    "[rename] target already exists and RENAME_NOREPLACE is set. new_path={:?}",
                    new_path
                );
                reply.error(err.into());
                return;
            }
            if old_path == new_path {
                reply.ok();
                debug!(
                    "[rename] ok: old_path and new_path are the same. No action needed. path={:?}",
                    old_path
                );
                return;
            }
            let old_stat = match self.sftp.lstat(&old_path) {
                Ok(s) => s,
                Err(e) => {
                    let err = Error::from(e);
                    warn!(
                        "[rename] failed to get attributes for old path {:?}. err={}",
                        old_path, err
                    );
                    reply.error(err.into());
                    return;
                }
            };
            // POSIXの型の組み合わせ規則
            if old_stat.is_dir() && !new_stat.is_dir() {
                let err = Error(Errno::ENOTDIR);
                warn!(
                    "[rename] cannot rename a directory to a non-directory. old_path={:?}, new_path={:?}",
                    old_path, new_path
                );
                reply.error(err.into());
                return;
            }
            if !old_stat.is_dir() && new_stat.is_dir() {
                let err = Error(Errno::EISDIR);
                warn!(
                    "[rename] cannot rename a non-directory to a directory. old_path={:?}, new_path={:?}",
                    old_path, new_path
                );
                reply.error(err.into());
                return;
            }
            if new_stat.is_dir() {
                // 空でなければENOTEMPTYで失敗する(POSIXと同じ)。
                if let Err(e) = self.sftp.rmdir(&new_path) {
                    let err = Error(Self::rmdir_errno(e));
                    warn!(
                        "[rename] failed to remove existing directory {:?} before rename. err={}",
                        new_path, err
                    );
                    reply.error(err.into());
                    return;
                }
            } else if let Err(e) = self.sftp.unlink(&new_path) {
                let err = Error::from(e);
                warn!(
                    "[rename] failed to remove existing file {:?} before rename. err={}",
                    new_path, err
                );
                reply.error(err.into());
                return;
            }
            self.inodes.del_inode_with_path(&new_path);
        }

        match self
            .sftp
            .rename(&old_path, &new_path, Some(ssh2::RenameFlags::NATIVE))
        {
            Ok(_) => {
                self.inodes.rename(&old_path, &new_path);
                reply.ok();
                debug!("[rename] Ok: renamed {:?} to {:?}", old_path, new_path);
            }
            Err(e) => {
                let err = Error::from(e);
                warn!(
                    "[rename] failed to rename {:?} to {:?}. err={}",
                    old_path, new_path, err
                );
                reply.error(err.into());
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Error(Errno);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Error({})",
            std::io::Error::from_raw_os_error(self.0.code())
        )
    }
}

impl From<Error> for Errno {
    fn from(value: Error) -> Self {
        value.0
    }
}

impl AsRef<Errno> for Error {
    fn as_ref(&self) -> &Errno {
        &self.0
    }
}

impl From<ssh2::Error> for Error {
    fn from(value: ssh2::Error) -> Self {
        let eno = match value.code() {
            ssh2::ErrorCode::Session(_) => Errno::ENXIO,
            ssh2::ErrorCode::SFTP(i) => match i {
                // libssh2のlibssh2_sftp.hにて定義されている。
                2 => Errno::ENOENT,        // NO_SUCH_FILE
                3 => Errno::EACCES,        // permission_denied
                4 => Errno::EIO,           // failure
                5 => Errno::ENODEV,        // bad message
                6 => Errno::ENXIO,         // no connection
                7 => Errno::ENETDOWN,      // connection lost
                8 => Errno::ENODEV,        // unsported
                9 => Errno::EBADF,         // invalid handle
                10 => Errno::ENOENT,       //no such path
                11 => Errno::EEXIST,       // file already exists
                12 => Errno::EACCES,       // write protected
                13 => Errno::ENXIO,        // no media
                14 => Errno::ENOSPC,       // no space on filesystem
                15 => Errno::EDQUOT,       // quota exceeded
                16 => Errno::ENODEV,       // unknown principal
                17 => Errno::ENOLCK,       // lock conflict
                18 => Errno::ENOTEMPTY,    // dir not empty
                19 => Errno::ENOTDIR,      // not a directory
                20 => Errno::ENAMETOOLONG, // invalid file name
                21 => Errno::ELOOP,        // link loop
                _ => {
                    error!("An unknown error occurred during SSH2.[{}]", i);
                    Errno::EIO
                }
            },
        };
        Self(eno)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        let eno = match value.kind() {
            NotFound => Errno::ENOENT,
            PermissionDenied => Errno::EACCES,
            ConnectionRefused => Errno::ECONNREFUSED,
            ConnectionReset => Errno::ECONNRESET,
            ConnectionAborted => Errno::ECONNABORTED,
            NotConnected => Errno::ENOTCONN,
            AddrInUse => Errno::EADDRINUSE,
            AddrNotAvailable => Errno::EADDRNOTAVAIL,
            BrokenPipe => Errno::EPIPE,
            AlreadyExists => Errno::EEXIST,
            WouldBlock => Errno::EWOULDBLOCK,
            InvalidInput => Errno::EINVAL,
            InvalidData => Errno::EILSEQ,
            TimedOut => Errno::ETIMEDOUT,
            WriteZero => Errno::EIO,
            Interrupted => Errno::EINTR,
            Unsupported => Errno::ENOTSUP,
            UnexpectedEof => Errno::EFAULT,
            OutOfMemory => Errno::ENOMEM,
            _ => {
                error!(
                    "An unknown error occurred during std::io.[{}]",
                    value.kind()
                );
                Errno::EIO
            }
        };
        Self(eno)
    }
}
