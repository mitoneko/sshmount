//! ファイルハンドル管理モジュール

use std::collections::HashMap;

/// ファイルハンドル管理構造体
pub(super) struct Fhandles {
    list: HashMap<u64, ssh2::File>,
    next_handle: u64,
}

impl Fhandles {
    pub(super) fn new() -> Self {
        Self {
            list: HashMap::new(),
            next_handle: 0,
        }
    }

    pub(super) fn add_file(&mut self, file: ssh2::File) -> u64 {
        let handle = self.next_handle;
        self.list.insert(handle, file);
        self.next_handle += 1;
        handle
    }

    pub(super) fn get_file(&mut self, fh: u64) -> Option<&mut ssh2::File> {
        self.list.get_mut(&fh)
    }

    pub(super) fn del_file(&mut self, fh: u64) {
        self.list.remove(&fh); // 戻り値は捨てる。この時点でファイルはクローズ。
                               // ハンドルの再利用のため、次回ハンドルを調整
        match self.list.keys().max() {
            Some(&i) => self.next_handle = i + 1,
            None => self.next_handle = 0,
        }
    }
}
