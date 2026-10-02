//! ファイルハンドル管理モジュール

use std::collections::HashMap;
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
