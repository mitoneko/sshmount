//! bidirectional hash map
//! 双方向ハッシュマップ

use std::{collections::HashMap, hash::Hash, sync::Arc};

/// 双方向ハッシュマップ
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct BiHashMap<L, R>
where
    L: Hash + Eq + Clone,
    R: Hash + Eq + Clone,
{
    left: HashMap<Arc<L>, Arc<R>>,
    right: HashMap<Arc<R>, Arc<L>>,
}

impl<L, R> BiHashMap<L, R>
where
    L: Hash + Eq + Clone,
    R: Hash + Eq + Clone,
{
    /// 新しい双方向ハッシュマップを生成する
    pub fn new() -> Self {
        BiHashMap {
            right: HashMap::new(),
            left: HashMap::new(),
        }
    }

    /// チェック無しで挿入する。
    /// この時点で与えられる引数は、R,Lのいずれも既存のキーと重複しないことが保証されている必要がある。
    fn insert_no_check(&mut self, left: L, right: R) {
        let right = Arc::new(right);
        let left = Arc::new(left);
        self.right.insert(right.clone(), left.clone());
        self.left.insert(left, right);
    }

    /// マップに新しい要素を挿入する。
    /// 既存のキーと重複する場合、対応する値を上書きし、OverwriteResultで通知する。
    pub fn insert(&mut self, left: L, right: R) -> OverwriteResult<L, R> {
        let old_left = self.right.remove(&right);
        let old_right = self.left.remove(&left);
        let result = match (old_left, old_right) {
            (None, None) => OverwriteResult::NoOverwrite,
            (Some(old_l), None) => {
                self.left.remove(old_l.as_ref());
                OverwriteResult::OverwriteLeft((*old_l).clone())
            }
            (None, Some(old_r)) => {
                self.right.remove(old_r.as_ref());
                OverwriteResult::OverwriteRight((*old_r).clone())
            }
            (Some(old_l), Some(old_r)) => {
                self.left.remove(old_l.as_ref());
                self.right.remove(old_r.as_ref());
                OverwriteResult::OverwriteBoth(
                    (left.clone(), (*old_r).clone()),
                    ((*old_l).clone(), right.clone()),
                )
            }
        };
        self.insert_no_check(left, right);
        result
    }

    /// マップに新しい値を挿入する
    /// 既存のキーが存在する場合は、エラーを返す。
    pub fn insert_no_overwrite(&mut self, left: L, right: R) -> Result<(), ()> {
        if self.contains_left(&left) || self.contains_right(&right) {
            return Err(());
        }
        self.insert_no_check(left, right);
        Ok(())
    }

    /// 左側のキーから右側の値を取得する
    /// 存在しない場合はNoneを返す
    pub fn get_right(&self, left: &L) -> Option<&R> {
        self.left.get(left).map(|arc_r| arc_r.as_ref())
    }

    /// 右側のキーから左側の値を取得する
    /// 存在しない場合はNoneを返す
    pub fn get_left(&self, right: &R) -> Option<&L> {
        self.right.get(right).map(|arc_l| arc_l.as_ref())
    }

    /// 左側のキーが存在するかどうかを返す
    /// 存在する場合はtrue、存在しない場合はfalseを返す
    pub fn contains_left(&self, left: &L) -> bool {
        self.left.contains_key(left)
    }

    /// 右側のキーが存在するかどうかを返す
    /// 存在する場合はtrue、存在しない場合はfalseを返す
    pub fn contains_right(&self, right: &R) -> bool {
        self.right.contains_key(right)
    }

    /// 左側の値から、リストの項目を削除する
    /// 存在しない場合は、なにもしない。
    /// 以前の値を返す。(存在しない場合はNone)
    pub fn remove_left(&mut self, left: &L) -> Option<R> {
        let result = self.left.remove(left).map(|arc_r| (*arc_r).clone());
        if let Some(right) = result.as_ref() {
            self.right.remove(right);
        };
        result
    }

    /// 右側の値から、リストの項目を削除する
    /// 存在しない場合は、なにもしない。
    /// 以前の値を返す。(存在しない場合はNone)
    pub fn remove_right(&mut self, right: &R) -> Option<L> {
        let result = self.right.remove(right).map(|arc_l| (*arc_l).clone());
        if let Some(left) = result.as_ref() {
            self.left.remove(left);
        };
        result
    }
}

/// 挿入時の上書き結果
#[derive(Debug, PartialEq, Eq)]
pub enum OverwriteResult<L, R> {
    NoOverwrite,
    OverwriteRight(R),
    OverwriteLeft(L),
    OverwriteBoth((L, R), (L, R)),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 単純な挿入と、値の取得のテスト
    fn tanjyunna_insert() {
        let mut bimap = BiHashMap::new();
        assert_eq!(bimap.insert(1, "a"), OverwriteResult::NoOverwrite);
        assert_eq!(bimap.get_right(&1), Some(&"a"));
        assert_eq!(bimap.get_left(&"a"), Some(&1));
        assert_eq!(bimap.insert_no_overwrite(2, "b"), Ok(()));
        assert_eq!(bimap.get_right(&2), Some(&"b"));
        assert_eq!(bimap.get_left(&"b"), Some(&2));
        assert!(bimap.contains_left(&1));
        assert!(bimap.contains_right(&"b"));
    }

    #[test]
    /// 上書き挿入のテスト
    /// 左側、右側、両側の上書きのケースを確認する
    fn overwrite_insert() {
        let mut bimap = BiHashMap::new();
        assert_eq!(bimap.insert(1, "a"), OverwriteResult::NoOverwrite);
        print_hash_map(&bimap, "insert (1, 'a')");
        assert_eq!(bimap.insert(1, "b"), OverwriteResult::OverwriteRight("a"));
        print_hash_map(&bimap, "insert (1, 'b')");
        assert_eq!(bimap.get_right(&1), Some(&"b"));
        assert_eq!(bimap.insert(2, "b"), OverwriteResult::OverwriteLeft(1));
        print_hash_map(&bimap, "insert (2, 'b')");
        assert_eq!(bimap.get_left(&"b"), Some(&2));
        assert_eq!(bimap.get_right(&2), Some(&"b"));
        assert_eq!(bimap.insert_no_overwrite(3, "c"), Ok(()));
        print_hash_map(&bimap, "insert (3, 'c')");
        assert_eq!(
            bimap.insert(2, "b"),
            OverwriteResult::OverwriteBoth((2, "b"), (2, "b"))
        );
        print_hash_map(&bimap, "insert (2, 'b') again");
        assert_eq!(
            bimap.insert(3, "b"),
            OverwriteResult::OverwriteBoth((3, "c"), (2, "b"))
        );
        print_hash_map(&bimap, "insert (3, 'b')");
        assert_eq!(bimap.get_right(&3), Some(&"b"));
        assert_eq!(bimap.get_left(&"b"), Some(&3));
        assert_eq!(bimap.left.len(), bimap.right.len());
        assert_eq!(bimap.get_right(&2), None);
        assert_eq!(bimap.insert_no_overwrite(3, "d"), Err(()));
        print_hash_map(&bimap, "insert_no_overwright (3, 'd') [fail]");
        assert_eq!(bimap.insert_no_overwrite(5, "e"), Ok(()));
        print_hash_map(&bimap, "insert_no_overwright (5, 'e') [ok]");
        assert_eq!(bimap.insert_no_overwrite(5, "d"), Err(()));
        print_hash_map(&bimap, "insert_no_overwright (5, 'd') [fail]");
    }

    /// 左右の削除のテスト
    #[test]
    fn remove_test() {
        let mut bimap = BiHashMap::new();
        bimap.insert_no_check(1, "a");
        bimap.insert_no_check(2, "b");
        bimap.insert_no_check(3, "c");
        print_hash_map(&bimap, "initial map");
        assert_eq!(bimap.remove_left(&2), Some("b"));
        print_hash_map(&bimap, "after remove_left(2)");
        assert_eq!(bimap.get_left(&"b"), None);
        assert_eq!(bimap.get_right(&2), None);
        assert_eq!(bimap.remove_right(&"c"), Some(3));
        print_hash_map(&bimap, "after remove_right('c')");
        assert_eq!(bimap.get_left(&"c"), None);
        assert_eq!(bimap.get_right(&3), None);
        assert_eq!(bimap.remove_left(&4), None);
        print_hash_map(&bimap, "after remove_left(4) [no op]");
        assert_eq!(bimap.remove_right(&"d"), None);
        print_hash_map(&bimap, "after remove_right('d') [no op]");
    }

    use std::fmt::Debug;
    fn print_hash_map<R, L>(bimap: &BiHashMap<R, L>, mes: &str)
    where
        R: Debug + Hash + Eq + Clone,
        L: Debug + Hash + Eq + Clone,
    {
        println!("=== {} ===", mes);
        println!("Left to Right:");
        for (l, r) in &bimap.left {
            println!("  {:?} => {:?}", l, r);
        }
        println!("Right to Left:");
        for (r, l) in &bimap.right {
            println!("  {:?} => {:?}", r, l);
        }
    }
}
