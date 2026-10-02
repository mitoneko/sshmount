use log::error;
use std::sync::{Mutex, MutexGuard};

/// Mutexの独自拡張
pub(super) trait MutexLockExt<T> {
    /// Mutexをロックし、毒化されていた場合は、ログを取得した上で即座にpanicする。
    fn lock_or_panic(&self, func: impl AsRef<str>, item: impl AsRef<str>) -> MutexGuard<'_, T>;
}

impl<T> MutexLockExt<T> for Mutex<T> {
    fn lock_or_panic(&self, func: impl AsRef<str>, item: impl AsRef<str>) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(_poisoned) => {
                let func = func.as_ref();
                let item = item.as_ref();
                error!(
                    "{}: 致命的エラー。{}のMutexが毒化された。panic!()する。",
                    func, item
                );
                panic!("{}: 致命的エラー。{}のMutexが毒化された。", func, item);
            }
        }
    }
}
