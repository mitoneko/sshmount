/// FUSE ファイルシステム実装
use fuser::{
    FileAttr, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use libc::ENOENT;
use log::debug;
use ssh2::{Session, Sftp};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
    io::{Seek, Read},
};

pub struct Sshfs {
    _session: Session,
    sftp: Sftp,
    inodes: Inodes,
    _top_path: PathBuf,
}

impl Sshfs {
    pub fn new(session: Session) -> Self {
        let mut inodes = Inodes::new();
        let top_path: PathBuf = Path::new("/home/mito").into();
        inodes.add(&top_path);
        let sftp = session.sftp().unwrap();
        Self {
            _session: session,
            sftp,
            inodes,
            _top_path: top_path,
        }
    }

    /// ssh2経由でファイルのステータスを取得する。
    /// 副作用:取得に成功した場合、inodesにパスを登録する。
    fn getattr_from_ssh2(
        &mut self,
        path: &Path,
        uid: u32,
        gid: u32,
    ) -> Result<FileAttr, Error> {
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

    fn conv_file_kind_ssh2fuser(filetype : &ssh2::FileType) -> Result<fuser::FileType, Error> {
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

    fn getattr(&mut self, req: &Request, ino: u64, reply: ReplyAttr) {
        let Some(path) = self.inodes.get_path(ino) else {
            reply.error(ENOENT);
            return;
        };
        match self.getattr_from_ssh2(&path, req.uid(), req.gid()) {
            Ok(attr) => reply.attr(&Duration::from_secs(1), &attr),
            Err(e) => reply.error(e.0),
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
                    mtime: None
                }; // "." ".."の解決用attr ディレクトリであることのみを示す。
                dir.insert(0, (Path::new("..").into(), cur_file_attr.clone()));
                dir.insert(0, (Path::new(".").into(), cur_file_attr));
                let mut i = offset+1;
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
                            debug!("[readdir]ファイルタイプ解析失敗: inode={}, name={:?}", ino, name);
                            reply.error(e.0);
                            return;
                        }
                    };
                    if reply.add(ino, i, filetype, name) {break;}
                    i += 1;
                }
                reply.ok();
            }
            Err(e) => {
                debug!("ssh2::readdir内でエラー発生");
                reply.error(Error::from(e).0);
            }
        };
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        let Some(path) = self.inodes.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.sftp.readlink(&path) {
            Ok(p) => reply.data(p.as_os_str().to_str().unwrap().as_bytes()),
            Err(e) => reply.error(Error::from(e).0),
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(path) = self.inodes.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.sftp.open(&path) {
            Ok(mut f) => {
                if let Err(e) = f.seek(std::io::SeekFrom::Start(offset as u64)) {
                    reply.error(Error::from(e).0);
                    return;
                }
                let mut buff = Vec::<u8>::new();
                buff.resize(size as usize, 0u8);
                let mut read_size : usize = 0;
                while read_size < size as usize {
                    match f.read(&mut buff[read_size..]) {
                        Ok(s) => {
                           if s == 0 {break;};
                           read_size += s;
                        }
                        Err(e) => {
                            reply.error(Error::from(e).0);
                            return;
                        }
                    }
                }
                reply.data(&buff);
            }
            Err(e) => {
                reply.error(Error::from(e).0);
            }
        }
    }
}

#[derive(Debug, Default)]
struct Inodes {
    list: std::collections::HashMap<u64, PathBuf>,
    max_inode: u64,
}

impl Inodes {
    /// Inodeを生成する
    fn new() -> Self {
        Self {
            list: std::collections::HashMap::new(),
            max_inode: 0,
        }
    }

    /// pathで指定されたinodeを生成し、登録する。
    /// すでにpathの登録が存在する場合、追加はせず、登録済みのinodeを返す。
    fn add(&mut self, path: &Path) -> u64 {
        match self.get_inode(path){
            Some(i) => i,
            None => {
                self.max_inode += 1;
                self.list.insert(self.max_inode, path.into());
                self.max_inode
            }
        }
    }

    /// pathからinodeを取得する
    fn get_inode(&self, path: &Path) -> Option<u64> {
        self.list.iter().find(|(_, p)| path == *p).map(|(i, _)| *i)
    }

    /// inodeからpathを取得する
    fn get_path(&self, inode: u64) -> Option<PathBuf> {
        self.list.get(&inode).map(|p| (*p).clone())
    }
}

#[derive(Debug, Clone, Copy)]
struct Error(i32);

impl From<ssh2::Error> for Error {
    fn from(value : ssh2::Error) -> Self {
        let eno = match value.code() {
            ssh2::ErrorCode::Session(_) => libc::ENXIO,
            ssh2::ErrorCode::SFTP(i) => 
                match i {
                    // libssh2のlibssh2_sftp.hにて定義されている。
                    2 => libc::ENOENT,  // NO_SUCH_FILE
                    3 => libc::EACCES,  // permission_denied
                    4 => libc::EIO,     // failure
                    5 => libc::ENODEV,  // bad message
                    6 => libc::ENXIO,   // no connection
                    7 => libc::ENETDOWN,// connection lost
                    8 => libc::ENODEV,  // unsported
                    9 => libc::EBADF,   // invalid handle
                    10 => libc::ENOENT, //no such path
                    11 => libc::EEXIST, // file already exists
                    12 => libc::EACCES, // write protected
                    13 => libc::ENXIO,  // no media
                    14 => libc::ENOSPC, // no space on filesystem
                    15 => libc::EDQUOT, // quota exceeded
                    16 => libc::ENODEV, // unknown principal
                    17 => libc::ENOLCK, // lock conflict
                    18 => libc::ENOTEMPTY, // dir not empty
                    19 => libc::ENOTDIR,// not a directory
                    20 => libc::ENAMETOOLONG,// invalid file name
                    21 => libc::ELOOP, // link loop
                    _ => 0,
                }
        };
        Self(eno)
    }
}
    
impl From<std::io::Error> for Error {
    fn from(value : std::io::Error) -> Self {
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
            _ => 0,
        };
        Self(eno)
    }
}

#[cfg(test)]
mod inode_test {
    use super::Inodes;
    use std::path::Path;

    #[test]
    fn inode_add_test() {
        let mut inodes = Inodes::new();
        assert_eq!(inodes.add(Path::new("")), 1);
        assert_eq!(inodes.add(Path::new("test")), 2);
        assert_eq!(inodes.add(Path::new("")), 1);
        assert_eq!(inodes.add(Path::new("test")), 2);
        assert_eq!(inodes.add(Path::new("test3")), 3);
        assert_eq!(inodes.add(Path::new("/test")), 4);
        assert_eq!(inodes.add(Path::new("test/")), 2);
    }

    fn make_inodes() -> Inodes {
        let mut inodes = Inodes::new();
        inodes.add(Path::new(""));
        inodes.add(Path::new("test"));
        inodes.add(Path::new("test2"));
        inodes.add(Path::new("test3/"));
        inodes
    }

    #[test]
    fn inodes_get_inode_test() {
        let inodes = make_inodes();
        assert_eq!(inodes.get_inode(Path::new("")), Some(1));
        assert_eq!(inodes.get_inode(Path::new("test4")), None);
        assert_eq!(inodes.get_inode(Path::new("/test")), None);
        assert_eq!(inodes.get_inode(Path::new("test3")), Some(4));
    }

    #[test]
    fn inodes_get_path_test() {
        let inodes = make_inodes();
        assert_eq!(inodes.get_path(1), Some(Path::new("").into()));
        assert_eq!(inodes.get_path(3), Some(Path::new("test2").into()));
        assert_eq!(inodes.get_path(5), None);
        assert_eq!(inodes.get_path(3), Some(Path::new("test2/").into()));
    }
}
