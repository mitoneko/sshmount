//! ファイルハンドル管理モジュール

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

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

    pub(super) fn add_file(&mut self, file: ssh2::File) -> u64 {
        let handle = self.next_handle.fetch_add(1, Ordering::AcqRel);
        self.list
            .lock()
            .unwrap()
            .insert(handle, Arc::new(Mutex::new(file)));
        handle
        // 注釈:このリストが毒化されたら、もはや、全システムにわたり、ファイル操作の正当性を保証できない。
        // プログラムとしてできることは即座にシステムを落とすことだけである。
        // よって、このモジュール内において、lock().unwrap()とする。
    }

    pub(super) fn get_file(&mut self, fh: u64) -> Option<Arc<Mutex<ssh2::File>>> {
        self.list.lock().unwrap().get_mut(&fh).cloned()
    }

    pub(super) fn del_file(&mut self, fh: u64) {
        self.list.lock().unwrap().remove(&fh); // 戻り値は捨てる。この時点でファイルはクローズ。
    }
}
