use std::path::PathBuf;

/// 環境変数 $SSH_TEST_MOUNT　の取得。
/// ここが、テスト時にファイル操作を行うルートディレクトリとなる。
///
///  # Errors
/// 環境変数がない場合、テスト実施不可として、Errを返す。
/// このテストは、cargo make test で実行さる前提である。
/// $SSH_TEST_MOUNTは、Makefile.tomlで定義されている。
pub fn get_test_root_dir() -> Result<PathBuf, EnvNotFound> {
    let env = std::env::var("SSH_TEST_MOUNT");
    match env {
        Ok(path) => Ok(PathBuf::from(path)),
        Err(_) => Err(EnvNotFound),
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Not found environment variable.This test can only be run using \"cargo make test\".")]
pub struct EnvNotFound;
