//! Inode管理モジュール

use super::bi_hash_map::BiHashMap;

use std::path::{Path, PathBuf};

/// Inode管理構造体
#[derive(Debug, Default)]
pub(super) struct Inodes {
    list: BiHashMap<u64, PathBuf>,
    max_inode: u64,
}

impl Inodes {
    /// Inodeを生成する
    pub(super) fn new() -> Self {
        Self {
            list: BiHashMap::new(),
            max_inode: 0,
        }
    }

    /// pathで指定されたinodeを生成し、登録する。
    /// すでにpathの登録が存在する場合、追加はせず、登録済みのinodeを返す。
    pub(super) fn add<P: AsRef<Path>>(&mut self, path: P) -> u64 {
        match self.get_inode(&path) {
            Some(i) => i,
            None => {
                self.max_inode += 1;
                let path = PathBuf::from(path.as_ref());
                if self
                    .list
                    .insert_no_overwrite(self.max_inode, path.clone())
                    .is_err()
                {
                    unreachable!(
                        "Unexpected duplicate inode {} or path {:?}",
                        self.max_inode, path
                    ); // 既に重複がチェックされているので、ありえない。
                }
                self.max_inode
            }
        }
    }

    /// pathからinodeを取得する
    pub(super) fn get_inode<P: AsRef<Path>>(&self, path: P) -> Option<u64> {
        let path = PathBuf::from(path.as_ref());
        self.list.get_left(&path).copied()
    }

    /// inodeからpathを取得する
    pub(super) fn get_path(&self, inode: u64) -> Option<PathBuf> {
        self.list.get_right(&inode).cloned()
    }

    /// inodesから、inodeの登録を削除する
    /// (主用途がなくなっちゃったけど、将来のために残しておく)
    #[allow(dead_code)]
    pub(super) fn del_inode(&mut self, inode: u64) -> Option<u64> {
        self.list.remove_left(&inode).map(|_| inode)
    }

    /// path名からiNodeの登録を削除する
    pub(super) fn del_inode_with_path<P: AsRef<Path>>(&mut self, path: P) -> Option<u64> {
        let path = PathBuf::from(path.as_ref());
        self.list.remove_right(&path)
    }

    /// 登録されているinodeのpathを変更する。
    /// old_pathが存在しなければ、なにもしない。
    pub(super) fn rename<P: AsRef<Path>>(&mut self, old_path: P, new_path: P) {
        let Some(ino) = self.get_inode(old_path) else {
            return;
        };
        self.list.remove_left(&ino);
        let new_path = PathBuf::from(new_path.as_ref());
        self.list.insert(ino, new_path);
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
        assert_eq!(inodes.list, inodes2.list);
    }
}
