mod bi_hash_map;
mod file_handle;
mod inode;

use file_handle::Fhandles;
use inode::Inodes;

use anyhow::Context;
use fuser::{FileAttr, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc::ENOENT;
use log::{debug, error, warn};
use ssh2::{ErrorCode, OpenFlags, OpenType, Session, Sftp};
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
    _top_path: PathBuf,
}

impl Sshfs {
    pub fn new<P: AsRef<Path>>(session: Session, path: P) -> anyhow::Result<Self> {
        let mut inodes = Inodes::new();
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
            &top_path, &inodes
        );
        Ok(Self {
            _session: session,
            sftp,
            inodes,
            fhandls: Fhandles::new(),
            _top_path: top_path,
        })
    }

    /// ssh2経由でファイルのステータスを取得する。
    /// 副作用:取得に成功した場合、inodesにパスを登録する。
    fn getattr_from_ssh2(&mut self, path: &Path, uid: u32, gid: u32) -> Result<FileAttr, Error> {
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
            ssh2::FileType::Other(_) => Err(Error(libc::EBADF)),
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
    fn lookup(&mut self, req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(mut path) = self.inodes.get_path(parent) else {
            debug!("[lookup] 親ディレクトリの検索に失敗 inode={}", parent);
            reply.error(ENOENT);
            return;
        };
        path.push(Path::new(name));
        match self.getattr_from_ssh2(&path, req.uid(), req.gid()) {
            Ok(attr) => reply.entry(&Duration::from_secs(1), &attr, 0),
            Err(e) => {
                reply.error(e.0);
            }
        };
    }

    fn getattr(&mut self, req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let Some(path) = self.inodes.get_path(ino) else {
            debug!("[getattr] path取得失敗: inode={}", ino);
            reply.error(ENOENT);
            return;
        };
        match self.getattr_from_ssh2(&path, req.uid(), req.gid()) {
            Ok(attr) => {
                //debug!("[getattr]retrun attr: {:?}", &attr);
                reply.attr(&Duration::from_secs(1), &attr);
            }
            Err(e) => {
                warn!("[getattr] getattr_from_ssh2エラー: {:?}", &e);
                reply.error(e.0)
            }
        };
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Some(path) = self.inodes.get_path(ino) else {
            reply.error(libc::ENOENT);
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
                let mut i = offset + 1;
                for f in dir.iter().skip(offset as usize) {
                    let ino = if f.0 == Path::new("..") || f.0 == Path::new(".") {
                        1
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
                                ino, name
                            );
                            reply.error(e.0);
                            return;
                        }
                    };
                    if reply.add(ino, i, filetype, name) {
                        break;
                    }
                    i += 1;
                }
                reply.ok();
            }
            Err(e) => {
                warn!("[readdir]ssh2::readdir内でエラー発生-- {:?}", e);
                reply.error(Error::from(e).0);
            }
        };
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        let Some(path) = self.inodes.get_path(ino) else {
            error!("[readlink] 親ディレクトリの検索に失敗 {ino}");
            reply.error(libc::ENOENT);
            return;
        };
        match self.sftp.readlink(&path) {
            Ok(p) => {
                //debug!("[readlink] ret_path => {:?}", &p);
                reply.data(p.as_os_str().to_string_lossy().as_bytes());
            }
            Err(e) => {
                //debug!("[readlink] ssh2::readlink error => {e:?}");
                reply.error(Error::from(e).0);
            }
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: fuser::ReplyOpen) {
        let Some(file_name) = self.inodes.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };

        let mut flags_ssh2 = OpenFlags::empty();
        if flags & libc::O_WRONLY != 0 {
            flags_ssh2.insert(OpenFlags::WRITE);
        } else if flags & libc::O_RDWR != 0 {
            flags_ssh2.insert(OpenFlags::READ);
            flags_ssh2.insert(OpenFlags::WRITE);
        } else {
            flags_ssh2.insert(OpenFlags::READ);
        }
        if flags & libc::O_APPEND != 0 {
            flags_ssh2.insert(OpenFlags::APPEND);
        }
        if flags & libc::O_CREAT != 0 {
            flags_ssh2.insert(OpenFlags::CREATE);
        }
        if flags & libc::O_TRUNC != 0 {
            flags_ssh2.insert(OpenFlags::TRUNCATE);
        }
        if flags & libc::O_EXCL != 0 {
            flags_ssh2.insert(OpenFlags::EXCLUSIVE);
        }

        debug!(
            "[open] filename='{:?}', openflag = {:?}, bit = {:x}",
            &file_name,
            &flags_ssh2,
            flags_ssh2.bits()
        );
        match self
            .sftp
            .open_mode(&file_name, flags_ssh2, 0o777, ssh2::OpenType::File)
        {
            Ok(file) => {
                let fh = self.fhandls.add_file(file);
                reply.opened(fh, flags as u32);
            }
            Err(e) => {
                log::error!(
                    "file-open error: filename='{:?}', mode={:?}, err={}",
                    &file_name,
                    &flags_ssh2,
                    &e
                );
                reply.error(Error::from(e).0);
            }
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        self.fhandls.del_file(fh);
        reply.ok();
    }

    fn read(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(file_mutex) = self.fhandls.get_file(fh) else {
            reply.error(libc::EINVAL);
            return;
        };
        let mut file = file_mutex.lock().unwrap();
        // 注釈:このデータの出所であるFhandles構造体内のデータに毒化があるということは、
        // システム全域の他のファイルハンドルの正当性も保証できないことを意味する。
        // ファイル操作を失敗させることより、システム全体を落とすことが正しい選択と思われる。
        // よって、lock().unwrap()とする。write()関数他においても同様。

        if let Err(e) = file.seek(std::io::SeekFrom::Start(offset as u64)) {
            reply.error(Error::from(e).0);
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
                    reply.error(Error::from(e).0);
                    return;
                }
            }
        }
        buff.resize(read_size, 0u8);
        reply.data(&buff);
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyWrite,
    ) {
        let Some(file_mutex) = self.fhandls.get_file(fh) else {
            reply.error(libc::EINVAL);
            return;
        };
        let file = &mut file_mutex.lock().unwrap();

        if let Err(e) = file.seek(std::io::SeekFrom::Start(offset as u64)) {
            reply.error(Error::from(e).0);
            return;
        }
        let mut buf = data;
        while !buf.is_empty() {
            let cnt = match file.write(buf) {
                Ok(cnt) => cnt,
                Err(e) => {
                    reply.error(Error::from(e).0);
                    return;
                }
            };
            buf = &buf[cnt..];
        }
        reply.written(data.len() as u32);
    }

    fn lseek(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        whence: i32,
        reply: fuser::ReplyLseek,
    ) {
        let seek_from = match whence {
            libc::SEEK_SET => SeekFrom::Start(offset as u64),
            libc::SEEK_CUR => SeekFrom::Current(offset),
            libc::SEEK_END => SeekFrom::End(offset),
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };
        let Some(file_mutex) = self.fhandls.get_file(fh) else {
            reply.error(libc::EBADF);
            return;
        };
        let file = &mut file_mutex.lock().unwrap();
        let pos = match file.seek(seek_from) {
            Ok(p) => p,
            Err(e) => {
                reply.error(Error::from(e).0);
                return;
            }
        };
        reply.offset(pos as i64);
    }

    fn mknod(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        if mode & libc::S_IFMT != libc::S_IFREG {
            reply.error(libc::EPERM);
            return;
        }
        let mode = mode & (!umask | libc::S_IFMT);
        let Some(mut new_name) = self.inodes.get_path(parent) else {
            reply.error(libc::ENOENT);
            return;
        };
        new_name.push(name);
        if let Err(e) =
            self.sftp
                .open_mode(&new_name, OpenFlags::CREATE, mode as i32, OpenType::File)
        {
            reply.error(Error::from(e).0);
            return;
        }
        let new_attr = match self.getattr_from_ssh2(&new_name, req.uid(), req.gid()) {
            Ok(a) => a,
            Err(e) => {
                reply.error(e.0);
                return;
            }
        };
        reply.entry(&Duration::from_secs(1), &new_attr, 0);
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        let Some(mut path) = self.inodes.get_path(parent) else {
            reply.error(libc::ENOENT);
            return;
        };
        path.push(name);
        match self.sftp.unlink(&path) {
            Ok(_) => {
                self.inodes.del_inode_with_path(&path);
                reply.ok();
            }
            Err(e) => reply.error(Error::from(e).0),
        }
    }

    fn mkdir(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        let Some(mut path) = self.inodes.get_path(parent) else {
            reply.error(libc::ENOENT);
            return;
        };
        path.push(name);

        let mode = (mode & (!umask) & 0o777) as i32;

        match self.sftp.mkdir(&path, mode) {
            Ok(_) => match self.getattr_from_ssh2(&path, req.uid(), req.gid()) {
                Ok(attr) => reply.entry(&Duration::from_secs(1), &attr, 0),
                Err(e) => reply.error(e.0),
            },
            Err(e) => reply.error(Error::from(e).0),
        }
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        let Some(mut path) = self.inodes.get_path(parent) else {
            reply.error(libc::ENOENT);
            return;
        };
        path.push(name);
        match self.sftp.rmdir(&path) {
            Ok(_) => {
                self.inodes.del_inode_with_path(&path);
                reply.ok()
            }
            Err(e) => {
                if e.code() == ErrorCode::Session(-31) {
                    // ssh2ライブラリの返すエラーが妙。置換しておく。
                    reply.error(libc::ENOTEMPTY);
                } else {
                    reply.error(Error::from(e).0)
                }
            }
        }
    }

    fn symlink(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        link: &Path,
        reply: ReplyEntry,
    ) {
        let Some(mut target) = self.inodes.get_path(parent) else {
            reply.error(libc::ENOENT);
            return;
        };
        target.push(name);
        match self.sftp.symlink(link, &target) {
            Ok(_) => match self.getattr_from_ssh2(&target, req.uid(), req.gid()) {
                Ok(attr) => reply.entry(&Duration::from_secs(1), &attr, 0),
                Err(e) => reply.error(e.0),
            },
            Err(e) => reply.error(Error::from(e).0),
        }
    }

    fn setattr(
        &mut self,
        req: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        atime: Option<fuser::TimeOrNow>,
        mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
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
            reply.error(ENOENT);
            return;
        };
        match self.sftp.setstat(&filename, stat) {
            Ok(_) => {
                let stat = self.getattr_from_ssh2(&filename, req.uid(), req.gid());
                match stat {
                    Ok(s) => reply.attr(&Duration::from_secs(1), &s),
                    Err(e) => reply.error(e.0),
                }
            }
            Err(e) => reply.error(Error::from(e).0),
        }
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        flags: u32,
        reply: fuser::ReplyEmpty,
    ) {
        let Some(mut old_path) = self.inodes.get_path(parent) else {
            reply.error(libc::ENOENT);
            return;
        };
        old_path.push(name);

        let Some(mut new_path) = self.inodes.get_path(newparent) else {
            reply.error(libc::ENOENT);
            return;
        };
        new_path.push(newname);

        let mut rename_flag = ssh2::RenameFlags::NATIVE;
        if flags & libc::RENAME_EXCHANGE != 0 {
            rename_flag.insert(ssh2::RenameFlags::ATOMIC);
        }
        if flags & libc::RENAME_NOREPLACE == 0 {
            // renameのOVERWRITEが効いてない。手動で消す。
            if let Ok(stat) = self.sftp.lstat(&new_path) {
                if stat.is_dir() {
                    if let Err(e) = self.sftp.rmdir(&new_path) {
                        reply.error(Error::from(e).0);
                        return;
                    }
                } else if let Err(e) = self.sftp.unlink(&new_path) {
                    reply.error(Error::from(e).0);
                    return;
                }
                self.inodes.del_inode_with_path(&new_path);
            }
        }

        match self.sftp.rename(&old_path, &new_path, Some(rename_flag)) {
            Ok(_) => {
                self.inodes.rename(&old_path, &new_path);
                reply.ok();
            }
            Err(e) => reply.error(Error::from(e).0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Error(i32);

impl From<ssh2::Error> for Error {
    fn from(value: ssh2::Error) -> Self {
        let eno = match value.code() {
            ssh2::ErrorCode::Session(_) => libc::ENXIO,
            ssh2::ErrorCode::SFTP(i) => match i {
                // libssh2のlibssh2_sftp.hにて定義されている。
                2 => libc::ENOENT,        // NO_SUCH_FILE
                3 => libc::EACCES,        // permission_denied
                4 => libc::EIO,           // failure
                5 => libc::ENODEV,        // bad message
                6 => libc::ENXIO,         // no connection
                7 => libc::ENETDOWN,      // connection lost
                8 => libc::ENODEV,        // unsported
                9 => libc::EBADF,         // invalid handle
                10 => libc::ENOENT,       //no such path
                11 => libc::EEXIST,       // file already exists
                12 => libc::EACCES,       // write protected
                13 => libc::ENXIO,        // no media
                14 => libc::ENOSPC,       // no space on filesystem
                15 => libc::EDQUOT,       // quota exceeded
                16 => libc::ENODEV,       // unknown principal
                17 => libc::ENOLCK,       // lock conflict
                18 => libc::ENOTEMPTY,    // dir not empty
                19 => libc::ENOTDIR,      // not a directory
                20 => libc::ENAMETOOLONG, // invalid file name
                21 => libc::ELOOP,        // link loop
                _ => {
                    error!("An unknown error occurred during SSH2.[{}]", i);
                    libc::EIO
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
            NotFound => libc::ENOENT,
            PermissionDenied => libc::EACCES,
            ConnectionRefused => libc::ECONNREFUSED,
            ConnectionReset => libc::ECONNRESET,
            ConnectionAborted => libc::ECONNABORTED,
            NotConnected => libc::ENOTCONN,
            AddrInUse => libc::EADDRINUSE,
            AddrNotAvailable => libc::EADDRNOTAVAIL,
            BrokenPipe => libc::EPIPE,
            AlreadyExists => libc::EEXIST,
            WouldBlock => libc::EWOULDBLOCK,
            InvalidInput => libc::EINVAL,
            InvalidData => libc::EILSEQ,
            TimedOut => libc::ETIMEDOUT,
            WriteZero => libc::EIO,
            Interrupted => libc::EINTR,
            Unsupported => libc::ENOTSUP,
            UnexpectedEof => libc::EOF,
            OutOfMemory => libc::ENOMEM,
            _ => {
                error!(
                    "An unknown error occurred during std::io.[{}]",
                    value.kind()
                );
                libc::EIO
            }
        };
        Self(eno)
    }
}
