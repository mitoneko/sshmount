//! Inode管理モジュール

use super::bi_hash_map::BiHashMap;

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
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
            next_inode: AtomicU64::new(2),
        }
    }

    /// pathで指定されたinodeを生成し、登録する。
    /// すでにpathの登録が存在する場合、追加はせず、登録済みのinodeを返す。
    /// 初めて、この関数が呼ばれるときは、ファイルシステムにおけるルートであり、inode番号2が割り当てられる。
    pub(super) fn add<P: AsRef<Path>>(&mut self, path: P) -> u64 {
        let mut list_guard = self.list.lock().unwrap();
        // 注釈:このリストが毒化されたら、もはや、全システムにわたり、inode管理の正当性を保証できない。
        // 最善の方法が、即時システムを落とすことである。
        // 以下、このモジュール全体に共通。
        let path = PathBuf::from(path.as_ref());
        match list_guard.get_left(&path) {
            Some(i) => *i,
            None => {
                let inode = self.next_inode.fetch_add(1, Ordering::AcqRel);
                if list_guard.insert_no_overwrite(inode, path.clone()).is_err() {
                    unreachable!("Unexpected duplicate inode {} or path {:?}", inode, path);
                    // 既に重複がチェックされているので、ありえない。
                }
                inode
            }
        }
    }

    /// pathからinodeを取得する
    /// 主用途が消滅したが、将来のために残しておく
    #[allow(dead_code)]
    pub(super) fn get_inode<P: AsRef<Path>>(&self, path: P) -> Option<u64> {
        let path = PathBuf::from(path.as_ref());
        self.list.lock().unwrap().get_left(&path).copied()
    }

    /// inodeからpathを取得する
    pub(super) fn get_path(&self, inode: u64) -> Option<PathBuf> {
        self.list.lock().unwrap().get_right(&inode).cloned()
    }

    /// inodesから、inodeの登録を削除する
    /// (主用途がなくなっちゃったけど、将来のために残しておく)
    #[allow(dead_code)]
    pub(super) fn del_inode(&mut self, inode: u64) -> Option<u64> {
        self.list.lock().unwrap().remove_left(&inode).map(|_| inode)
    }

    /// path名からiNodeの登録を削除する
    pub(super) fn del_inode_with_path<P: AsRef<Path>>(&mut self, path: P) -> Option<u64> {
        let path = PathBuf::from(path.as_ref());
        self.list.lock().unwrap().remove_right(&path)
    }

    /// 登録されているinodeのpathを変更する。
    /// old_pathが存在しなければ、なにもしない。
    pub(super) fn rename<P: AsRef<Path>>(&mut self, old_path: P, new_path: P) {
        let old_path = PathBuf::from(old_path.as_ref());
        let new_path = PathBuf::from(new_path.as_ref());
        let mut list_gaurd = self.list.lock().unwrap();
        let Some(ino) = list_gaurd.get_left(&old_path).copied() else {
            return;
        };
        list_gaurd.remove_left(&ino);
        list_gaurd.insert(ino, new_path);
    }
}

#[cfg(test)]
mod inode_test {
    use super::Inodes;
    use std::path::Path;

    #[test]
    fn inode_add_test() {
        let mut inodes = Inodes::new();
        assert_eq!(inodes.add(""), 2);
        assert_eq!(inodes.add(Path::new("test")), 3);
        assert_eq!(inodes.add(Path::new("")), 2);
        assert_eq!(inodes.add(Path::new("test")), 3);
        assert_eq!(inodes.add(Path::new("test3")), 4);
        assert_eq!(inodes.add(Path::new("/test")), 5);
        assert_eq!(inodes.add(Path::new("test/")), 3);
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
        assert_eq!(inodes.get_inode(Path::new("")), Some(2));
        assert_eq!(inodes.get_inode(Path::new("test4")), None);
        assert_eq!(inodes.get_inode(Path::new("/test")), None);
        assert_eq!(inodes.get_inode(Path::new("test3")), Some(5));
    }

    #[test]
    fn inodes_get_path_test() {
        let inodes = make_inodes();
        assert_eq!(inodes.get_path(2), Some(Path::new("").into()));
        assert_eq!(inodes.get_path(4), Some(Path::new("test2").into()));
        assert_eq!(inodes.get_path(6), None);
        assert_eq!(inodes.get_path(4), Some(Path::new("test2/").into()));
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
}
