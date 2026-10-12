//! Inode管理モジュール

use crate::ssh_filesystem::MutexLockExt;

use super::bi_hash_map::BiHashMap;

use fuser::INodeNo;
use std::path::{Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

/// Inode管理構造体
#[derive(Debug, Default)]
pub(super) struct Inodes {
    list: Mutex<BiHashMap<u64, PathBuf>>,
    next_inode: AtomicU64,
}

impl Inodes {
    /// Inodesを生成する
    pub(super) fn new() -> Self {
        Self {
            list: Mutex::new(BiHashMap::new()),
            next_inode: AtomicU64::new(1),
        }
    }

    /// pathで指定されたinodeを生成し、登録する。
    /// すでにpathの登録が存在する場合、追加はせず、登録済みのinodeを返す。
    /// 初めて、この関数が呼ばれるときは、ファイルシステムにおけるルートであり、inode番号1が割り当てられる。
    pub(super) fn add<P: AsRef<Path>>(&mut self, path: P) -> INodeNo {
        let mut list_guard = self.list.lock_or_panic("inodes::add", "inode list");
        let path = PathBuf::from(path.as_ref());
        match list_guard.get_left(&path) {
            Some(i) => INodeNo(*i),
            None => {
                let inode = self.next_inode.fetch_add(1, Ordering::AcqRel);
                if list_guard.insert_no_overwrite(inode, path.clone()).is_err() {
                    unreachable!("Unexpected duplicate inode {} or path {:?}", inode, path);
                    // 既に重複がチェックされているので、ありえない。
                }
                INodeNo(inode)
            }
        }
    }

    /// pathからinodeを取得する
    /// 主用途が消滅したが、将来のために残しておく
    #[allow(dead_code)]
    pub(super) fn get_inode<P: AsRef<Path>>(&self, path: P) -> Option<INodeNo> {
        let path = PathBuf::from(path.as_ref());
        self.list
            .lock_or_panic("Inodes::get_inode", "inode list")
            .get_left(&path)
            .copied()
            .map(INodeNo)
    }

    /// inodeからpathを取得する
    pub(super) fn get_path(&self, inode: INodeNo) -> Option<PathBuf> {
        self.list
            .lock_or_panic("Inodes::get_path", "inode list")
            .get_right(&inode.into())
            .cloned()
    }

    /// inodesから、inodeの登録を削除する
    /// (主用途がなくなっちゃったけど、将来のために残しておく)
    #[allow(dead_code)]
    pub(super) fn del_inode(&mut self, inode: INodeNo) -> Option<INodeNo> {
        self.list
            .lock_or_panic("Inodes::del_inode", "inode list")
            .remove_left(&inode.into())
            .map(|_| inode)
    }

    /// path名からiNodeの登録を削除する
    pub(super) fn del_inode_with_path<P: AsRef<Path>>(&mut self, path: P) -> Option<INodeNo> {
        let path = PathBuf::from(path.as_ref());
        self.list
            .lock_or_panic("Inodes::del_inode_with_path", "inode list")
            .remove_right(&path)
            .map(INodeNo)
    }

    /// 登録されているinodeのpathを変更する。
    /// old_pathが存在しなければ、なにもしない。
    pub(super) fn rename<P: AsRef<Path>>(&mut self, old_path: P, new_path: P) {
        let old_path = PathBuf::from(old_path.as_ref());
        let new_path = PathBuf::from(new_path.as_ref());
        let mut list_guard = self.list.lock_or_panic("Inodes::rename", "inode list");
        let Some(ino) = list_guard.get_left(&old_path).copied() else {
            return;
        };
        list_guard.remove_left(&ino);
        list_guard.insert(ino, new_path.clone());
        // ディレクトリ配下のファイルのパスを変更する。
        // old_pathがファイルだったとしても、strip_prefixが全件一致しないだけだから問題なし。
        // old_path本体は、すでに変更されているので、ここでは一致しない。
        let need_update_files: Vec<(u64, PathBuf)> = list_guard
            .iter()
            .filter_map(|(ino, path)| {
                let suffix = path.strip_prefix(&old_path).ok()?;
                Some((*ino, new_path.join(suffix)))
            })
            .collect();
        for (ino, new_path) in need_update_files {
            list_guard.remove_left(&ino);
            list_guard.insert(ino, new_path);
        }
    }
}

#[cfg(test)]
mod inode_test {
    use super::Inodes;
    use fuser::INodeNo;
    use std::path::Path;

    #[test]
    fn inode_add_test() {
        let mut inodes = Inodes::new();
        assert_eq!(inodes.add(""), INodeNo(1));
        assert_eq!(inodes.add(Path::new("test")), INodeNo(2));
        assert_eq!(inodes.add(Path::new("")), INodeNo(1));
        assert_eq!(inodes.add(Path::new("test")), INodeNo(2));
        assert_eq!(inodes.add(Path::new("test3")), INodeNo(3));
        assert_eq!(inodes.add(Path::new("/test")), INodeNo(4));
        assert_eq!(inodes.add(Path::new("test/")), INodeNo(2));
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
        assert_eq!(inodes.get_inode(Path::new("")), Some(INodeNo(1)));
        assert_eq!(inodes.get_inode(Path::new("test4")), None);
        assert_eq!(inodes.get_inode(Path::new("/test")), None);
        assert_eq!(inodes.get_inode(Path::new("test3")), Some(INodeNo(4)));
    }

    #[test]
    fn inodes_get_path_test() {
        let inodes = make_inodes();
        assert_eq!(inodes.get_path(INodeNo(1)), Some(Path::new("").into()));
        assert_eq!(inodes.get_path(INodeNo(3)), Some(Path::new("test2").into()));
        assert_eq!(inodes.get_path(INodeNo(5)), None);
        assert_eq!(
            inodes.get_path(INodeNo(3)),
            Some(Path::new("test2/").into())
        );
    }

    #[test]
    fn inodes_rename() {
        let mut inodes = make_inodes();
        let old = Path::new("test2");
        let new = Path::new("new_test");
        let ino = inodes.get_inode(old).unwrap();
        inodes.rename(old, new);
        assert_eq!(inodes.get_path(ino), Some(new.into()));

        let mut inodes = make_inodes();
        let inodes2 = make_inodes();
        inodes.rename(Path::new("nai"), Path::new("kawattenai"));
        assert_eq!(*inodes.list.lock().unwrap(), *inodes2.list.lock().unwrap());
    }

    #[test]
    fn inodes_rename_dir() {
        let mut inodes = make_inodes();
        let old_dir = Path::new("test3");
        let dir_inode = inodes.get_inode(old_dir).unwrap();
        let inode1 = inodes.add(old_dir.join("file1"));
        let inode2 = inodes.add(old_dir.join("file2"));
        let new_dir = Path::new("new_test3");
        inodes.rename(old_dir, new_dir);
        assert_eq!(
            inodes.get_path(dir_inode),
            Some(new_dir.into()),
            "ディレクトリのinodeのパスがおかしい"
        );
        let new_file1 = inodes.get_path(inode1).unwrap();
        let new_file2 = inodes.get_path(inode2).unwrap();
        assert_eq!(
            new_file1,
            new_dir.join("file1"),
            "ファイル1のパスがおかしい"
        );
        assert_eq!(
            new_file2,
            new_dir.join("file2"),
            "ファイル2のパスがおかしい"
        );
        assert_eq!(
            inodes.get_inode(new_dir.join("file1")),
            Some(inode1),
            "逆引きしたinodeが一致しない"
        );
        assert_eq!(
            inodes.get_inode(new_dir.join("file2")),
            Some(inode2),
            "逆引きしたinodeが一致しない"
        );
        assert_eq!(
            inodes.get_inode(old_dir.join("file1")),
            None,
            "古いパスで逆引きできてしまう"
        );
        assert_eq!(
            inodes.get_inode(old_dir.join("file2")),
            None,
            "古いパスで逆引きできてしまう"
        );
    }
}
