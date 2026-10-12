//! `sshmount` を実際に起動して、リモート上のファイルアクセスを検証する統合テスト。
//!
//! このテストは `cargo make test` での実行を前提とする。
//! 環境変数は `test/test.env` から `cargo-make` により読み込まれ、
//! テスト前に `sshmount` のビルド・接続準備・起動、テスト後に接続解放が行われる。
//!
//! 外部環境に依存するため、通常の `cargo test` では実行せず、各テストは `#[ignore]` を付けて運用する。
mod common;

use std::{fs, io::Write};

use common::get_test_root_dir;

/// [01]ファイルを作成し、存在を確認し、削除して、存在しないことを確認する。
#[test]
#[ignore]
fn test_create_and_delete_file() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let branch_num = uuid::Uuid::now_v7().to_string();
    let test_file_path = root_dir.join(format!("test_01_{}", branch_num));
    assert!(
        !test_file_path.try_exists()?,
        "まだ作ってないファイルが存在する。"
    );
    fs::File::create(&test_file_path)?;
    assert!(
        test_file_path.try_exists()?,
        "作成したファイルが存在しない。"
    );
    fs::remove_file(&test_file_path)?;
    assert!(
        !test_file_path.try_exists()?,
        "削除したファイルが存在する。"
    );
    Ok(())
}

/// [02]ファイルを作成し、ファイル名に使ったUUIDを保存。ファイルを開き直して、UUIDを読み出して一致を確認する。
#[test]
#[ignore]
fn test_write_and_read_file() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let branch_num = uuid::Uuid::now_v7().to_string();
    let test_file_path = root_dir.join(format!("test_02_{}", branch_num));
    {
        let mut file = fs::File::create(&test_file_path)?;
        use std::io::Write;
        file.write_all(branch_num.as_bytes())?;
    }
    {
        let content = fs::read_to_string(&test_file_path)?;
        assert_eq!(
            content, branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }
    fs::remove_file(&test_file_path)?;
    Ok(())
}

/// [03]ファイルを作成し、renameして、一応内容も確認する
#[test]
#[ignore]
fn test_rename_file() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let branch_num = uuid::Uuid::now_v7().to_string();
    let test_file_path = root_dir.join(format!("test_03_{}", branch_num));
    let renamed_file_path = root_dir.join(format!("test_03_renamed_{}", branch_num));
    {
        let mut file = fs::File::create(&test_file_path)?;
        use std::io::Write;
        file.write_all(branch_num.as_bytes())?;
    }
    fs::rename(&test_file_path, &renamed_file_path)?;
    {
        let content = fs::read_to_string(&renamed_file_path)?;
        assert_eq!(
            content, branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }
    fs::remove_file(&renamed_file_path)?;
    Ok(())
}

/// [04]ディレクトリの作成と削除
/// - ディレクトリを作成し、存在の確認
/// - デイレクトリ内にファイルを作成し、内容の確認
/// - ここでディレクトリを削除してみて、エラーになることを確認
/// - ディレクトリ内のファイルを削除
/// - ディレクトリを削除し、不存在を確認
#[test]
#[ignore]
fn test_create_and_delete_dir() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let dir_branch_num = uuid::Uuid::now_v7().to_string();
    let test_dir_path = root_dir.join(format!("test_04_dir_{}", dir_branch_num));
    let file_branch_num = uuid::Uuid::now_v7().to_string();
    let test_file_path = test_dir_path.join(format!("test_04_file_{}", file_branch_num));
    // ディレクトリの作成と確認
    fs::create_dir(&test_dir_path)?;
    assert!(
        test_dir_path.try_exists()?,
        "作成したディレクトリが存在しない。"
    );
    assert!(
        test_dir_path.is_dir(),
        "作成したものがディレクトリではない。"
    );
    // ファイルの作成と書き込み
    {
        let mut file = fs::File::create(&test_file_path)?;
        file.write_all(file_branch_num.as_bytes())?;
    }
    {
        let content = fs::read_to_string(&test_file_path)?;
        assert_eq!(
            content, file_branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }
    // ディレクトリの削除失敗の確認
    let remove_dir_err = fs::remove_dir(&test_dir_path).expect_err("削除できちゃだめ^^;");
    assert_eq!(
        remove_dir_err.kind(),
        std::io::ErrorKind::DirectoryNotEmpty,
        "エラー内容が想定外[{}]",
        remove_dir_err
    );
    // ディレクトリの削除は中身を消してからね。
    fs::remove_file(&test_file_path)?;
    fs::remove_dir(&test_dir_path)?;
    assert!(
        !test_dir_path.try_exists()?,
        "削除したディレクトリが存在する。"
    );
    Ok(())
}

/// [05]ディレクトリのrename
/// ディレクトリの名前を変更し、その配下のファイルの内容も確認する。
#[test]
#[ignore]
fn test_rename_dir() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let dir_branch_num = uuid::Uuid::now_v7().to_string();
    let test_dir_path = root_dir.join(format!("test_05_dir_{}", dir_branch_num));
    let renamed_dir_path = root_dir.join(format!("test_05_renamed_dir_{}", dir_branch_num));
    let file_branch_num = uuid::Uuid::now_v7().to_string();
    let test_file_path = test_dir_path.join(format!("test_05_file_{}", file_branch_num));
    let renamed_file_path = renamed_dir_path.join(format!("test_05_file_{}", file_branch_num));

    fs::create_dir(&test_dir_path)?;
    {
        let mut file = fs::File::create(&test_file_path)?;
        file.write_all(file_branch_num.as_bytes())?;
    }
    fs::rename(&test_dir_path, &renamed_dir_path)?;
    assert!(!test_dir_path.try_exists()?, "元のディレクトリが存在する。");
    assert!(
        renamed_dir_path.try_exists()?,
        "リネーム後のディレクトリが存在しない。"
    );
    {
        assert!(
            renamed_file_path.try_exists()?,
            "リネーム後のディレクトリ内のファイルが存在しない。"
        );
        let content = fs::read_to_string(&renamed_file_path)?;
        assert_eq!(
            content, file_branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }
    fs::remove_dir_all(renamed_dir_path)?;
    Ok(())
}
