//! ファイルハンドル管理モジュール

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use super::mutex_lock_ext::MutexLockExt;

/// ファイルハンドル管理構造体
pub(super) struct Fhandles {
    list: Mutex<HashMap<u64, Arc<Mutex<ssh2::File>>>>,
    next_handle: AtomicU64,
}

impl Fhandles {
    pub(super) fn new() -> Self {
        Self {
            list: Mutex::new(HashMap::new()),
            next_handle: AtomicU64::new(0),
        }
    }

    pub(super) fn add_file(&self, file: ssh2::File) -> u64 {
        let handle = self.next_handle.fetch_add(1, Ordering::AcqRel);
        self.list
            .lock_or_panic("fhandles::add_file", "handle list")
            .insert(handle, Arc::new(Mutex::new(file)));
        handle
    }

    pub(super) fn get_file(&self, fh: u64) -> Option<Arc<Mutex<ssh2::File>>> {
        self.list
            .lock_or_panic("fhandles::get_file", "handle list")
            .get(&fh)
            .cloned()
    }

    pub(super) fn del_file(&self, fh: u64) {
        self.list
            .lock_or_panic("fhandles::del_file", "handle list")
            .remove(&fh); // 戻り値は捨てる。この時点でファイルはクローズ。
    }
}

/// opendir時点のディレクトリ内容(スナップショット)。"."と".."を含む。
pub(super) type DirEntries = Vec<(PathBuf, ssh2::FileStat)>;

/// ディレクトリハンドル管理構造体
/// readdirのたびにSFTPから一覧を取り直すと、読み出し途中のunlink等で
/// offsetがずれてエントリが欠落する。そのため、opendir時の一覧を保持して使い回す。
pub(super) struct Dhandles {
    list: Mutex<HashMap<u64, Arc<DirEntries>>>,
    next_handle: AtomicU64,
}

impl Dhandles {
    pub(super) fn new() -> Self {
        Self {
            list: Mutex::new(HashMap::new()),
            next_handle: AtomicU64::new(0),
        }
    }

    pub(super) fn add_dir(&self, entries: DirEntries) -> u64 {
        let handle = self.next_handle.fetch_add(1, Ordering::AcqRel);
        self.list
            .lock_or_panic("dhandles::add_dir", "dir handle list")
            .insert(handle, Arc::new(entries));
        handle
    }

    pub(super) fn get_dir(&self, fh: u64) -> Option<Arc<DirEntries>> {
        self.list
            .lock_or_panic("dhandles::get_dir", "dir handle list")
            .get(&fh)
            .cloned()
    }

    pub(super) fn del_dir(&self, fh: u64) {
        self.list
            .lock_or_panic("dhandles::del_dir", "dir handle list")
            .remove(&fh);
    }
}
