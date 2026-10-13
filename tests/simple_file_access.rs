//! `sshmount` を実際に起動して、リモート上のファイルアクセスを検証する統合テスト。
//!
//! このテストは `cargo make test` での実行を前提とする。
//! 環境変数は `test/test.env` から `cargo-make` により読み込まれ、
//! テスト前に `sshmount` のビルド・接続準備・起動、テスト後に接続解放が行われる。
//!
//! 外部環境に依存するため、通常の `cargo test` では実行せず、各テストは `#[ignore]` を付けて運用する。
mod common;

use std::{
    fs::{self},
    io::Write,
    path::Path,
    sync::{Arc, Barrier},
};

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

/// [06]ディレクトリ内にディレクトリがあるときのrenameの挙動
#[test]
#[ignore]
fn test_rename_dir_with_subdir() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let dir_branch_num = uuid::Uuid::now_v7().to_string();
    let test_dir_path = root_dir.join(format!("test_06_dir_{}", dir_branch_num));
    let test_sub_dir_branch_num = uuid::Uuid::now_v7().to_string();
    let test_sub_dir_path =
        test_dir_path.join(format!("test_06_subdir_{}", test_sub_dir_branch_num));
    let test_file_branch_num = uuid::Uuid::now_v7().to_string();
    let test_file_path = test_sub_dir_path.join(format!("test_06_file_{}", test_file_branch_num));
    let renamed_dir_path = root_dir.join(format!("test_06_renamed_dir_{}", dir_branch_num));
    let renamed_sub_dir_path_sufix = test_sub_dir_path.strip_prefix(&test_dir_path).unwrap();
    let renamed_sub_dir_path = renamed_dir_path.join(renamed_sub_dir_path_sufix);
    let renamed_file_path_sufix = test_file_path.strip_prefix(&test_dir_path).unwrap();
    let renamed_file_path = renamed_dir_path.join(renamed_file_path_sufix);

    fs::create_dir(&test_dir_path)?;
    fs::create_dir(&test_sub_dir_path)?;
    {
        let mut file = fs::File::create(&test_file_path)?;
        file.write_all(test_file_branch_num.as_bytes())?;
    }

    assert!(
        test_file_path.try_exists()?,
        "作成したファイルが存在しない。"
    );

    fs::rename(&test_dir_path, &renamed_dir_path)?;

    assert!(
        renamed_sub_dir_path.try_exists()?,
        "リネーム後のディレクトリ内のサブディレクトリが存在しない。"
    );
    assert!(
        renamed_file_path.try_exists()?,
        "リネーム後のディレクトリ内のファイルが存在しない。"
    );
    {
        let content = fs::read_to_string(&renamed_file_path)?;
        assert_eq!(
            content, test_file_branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }

    fs::remove_dir_all(&renamed_dir_path)?;
    Ok(())
}

/// [07]変更後のディレクトリ名が既存のディレクトリ(空)と同じ場合の挙動
#[test]
#[ignore]
fn test_rename_dir_to_existing_empty_dir() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let dir_branch_num = uuid::Uuid::now_v7().to_string();
    let test_dir_path = root_dir.join(format!("test_07_dir_{}", dir_branch_num));
    let exist_dir_path = root_dir.join(format!("test_07_exist_dir_{}", dir_branch_num));

    fs::create_dir(&test_dir_path)?;
    fs::create_dir(&exist_dir_path)?;

    fs::rename(&test_dir_path, &exist_dir_path).expect("リネーム先が空なので成功するはず。");
    assert!(!test_dir_path.try_exists()?, "元のディレクトリが存在する。");
    assert!(
        exist_dir_path.try_exists()?,
        "リネーム後のディレクトリが存在しない。"
    );
    fs::remove_dir(&exist_dir_path)?;

    Ok(())
}

/// [08]変更後のディレクトリ名が既存のディレクトリ(空でない)と同じ場合の挙動
#[test]
#[ignore]
fn test_rename_dir_to_existing_nonempty_dir() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let dir_branch_num = uuid::Uuid::now_v7().to_string();
    let test_dir_path = root_dir.join(format!("test_08_dir_{}", dir_branch_num));
    let exist_dir_path = root_dir.join(format!("test_08_exist_dir_{}", dir_branch_num));
    let exist_dir_file_path = exist_dir_path.join(format!("test_08_exist_file_{}", dir_branch_num));

    fs::create_dir(&test_dir_path)?;
    fs::create_dir(&exist_dir_path)?;
    {
        let mut file = fs::File::create(&exist_dir_file_path)?;
        file.write_all(dir_branch_num.as_bytes())?;
    }

    let rename_err = fs::rename(&test_dir_path, &exist_dir_path)
        .expect_err("リネーム先が空でないので失敗するはず。");
    assert_eq!(
        rename_err.kind(),
        std::io::ErrorKind::DirectoryNotEmpty,
        "エラー内容が想定外[{}]",
        rename_err
    );

    fs::remove_dir(&test_dir_path)?;
    fs::remove_dir_all(&exist_dir_path)?;
    Ok(())
}

/// [09]ファイルのディレクトリ間ムーブの挙動
#[test]
#[ignore]
fn test_move_file_between_dirs() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let dir_branch_num = uuid::Uuid::now_v7().to_string();
    let from_dir_path = root_dir.join(format!("test_09_from_dir_{}", dir_branch_num));
    let to_dir_path = root_dir.join(format!("test_09_to_dir_{}", dir_branch_num));
    let test_file_path = from_dir_path.join(format!("test_09_file_{}", dir_branch_num));
    let moved_file_path = to_dir_path.join(format!("test_09_file_{}", dir_branch_num));

    fs::create_dir(&from_dir_path)?;
    fs::create_dir(&to_dir_path)?;
    {
        let mut file = fs::File::create(&test_file_path)?;
        file.write_all(dir_branch_num.as_bytes())?;
    }

    fs::rename(&test_file_path, &moved_file_path)?;
    assert!(!test_file_path.try_exists()?, "元のファイルが存在する。");
    assert!(
        moved_file_path.try_exists()?,
        "移動後のファイルが存在しない。"
    );
    {
        let content = fs::read_to_string(&moved_file_path)?;
        assert_eq!(
            content, dir_branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }

    fs::remove_dir(&from_dir_path)?;
    fs::remove_dir_all(&to_dir_path)?;

    Ok(())
}

/// [10]ディレクトリ間のディレクトリのムーブの挙動。
/// ちゃんと、配下のファイルも連れて行ってくれること。
#[test]
#[ignore]
fn test_move_dir_between_dirs() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let dir_branch_num = uuid::Uuid::now_v7().to_string();
    let from_dir_path = root_dir.join(format!("test_10_from_dir_{}", dir_branch_num));
    let to_dir_path = root_dir.join(format!("test_10_to_dir_{}", dir_branch_num));
    let sub_dir_path = from_dir_path.join(format!("test_10_subdir_{}", dir_branch_num));
    let test_file_path = sub_dir_path.join(format!("test_10_file_{}", dir_branch_num));
    let moved_dir_path = to_dir_path.join(format!("test_10_subdir_{}", dir_branch_num));
    let moved_file_path = moved_dir_path.join(format!("test_10_file_{}", dir_branch_num));

    fs::create_dir(&from_dir_path)?;
    fs::create_dir(&to_dir_path)?;
    fs::create_dir(&sub_dir_path)?;
    {
        let mut file = fs::File::create(&test_file_path)?;
        file.write_all(dir_branch_num.as_bytes())?;
    }

    fs::rename(&sub_dir_path, &moved_dir_path)?;
    assert!(!sub_dir_path.try_exists()?, "元のディレクトリが存在する。");
    assert!(
        moved_dir_path.try_exists()?,
        "移動後のディレクトリが存在しない。"
    );
    assert!(!test_file_path.try_exists()?, "元のファイルが存在する。");
    assert!(
        moved_file_path.try_exists()?,
        "移動後のファイルが存在しない。"
    );
    {
        let content = fs::read_to_string(&moved_file_path)?;
        assert_eq!(
            content, dir_branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }

    fs::remove_dir(&from_dir_path)?;
    fs::remove_dir_all(&to_dir_path)?;
    Ok(())
}

/// [11] ディレクトリから既存ファイル名へのrenameとその逆の挙動
#[test]
#[ignore]
fn test_rename_dir_to_existing_file() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = get_test_root_dir()?;
    let branch_num = uuid::Uuid::now_v7().to_string();
    let dir_path = root_dir.join(format!("test_11_dir_{}", branch_num));
    let file_path = root_dir.join(format!("test_11_file_{}", branch_num));

    fs::create_dir(&dir_path)?;
    {
        let mut file = fs::File::create(&file_path)?;
        file.write_all(branch_num.as_bytes())?;
    }

    let rename_err = fs::rename(&dir_path, &file_path)
        .expect_err("ディレクトリから既存ファイル名へのrenameは失敗するはず。");
    assert_eq!(
        rename_err.kind(),
        std::io::ErrorKind::NotADirectory,
        "エラー内容が想定外[{}]",
        rename_err
    );

    let rename_err = fs::rename(&file_path, &dir_path)
        .expect_err("既存ファイル名からディレクトリへのrenameは失敗するはず。");
    assert_eq!(
        rename_err.kind(),
        std::io::ErrorKind::IsADirectory,
        "エラー内容が想定外[{}]",
        rename_err
    );

    fs::remove_dir(&dir_path)?;
    fs::remove_file(&file_path)?;

    Ok(())
}

/// [12] ある程度の大きさのファイルを複数生成し、内容を確認する。
#[test]
#[ignore]
fn test_create_and_read_large_files() -> Result<(), Box<dyn std::error::Error>> {
    const FILE_SIZE: usize = 1024 * 1024;
    const FILE_COUNT: usize = 100;

    let root_dir = get_test_root_dir()?;
    let branch_num = uuid::Uuid::now_v7().to_string();
    let dir_path = root_dir.join(format!("test_12_dir_{}", branch_num));
    fs::create_dir(&dir_path)?;

    let mut file_paths = Vec::new();
    let mut hash = Vec::new();
    for i in 0..FILE_COUNT {
        let file_path = dir_path.join(format!("test_12_file_{}_{}", i, branch_num));
        file_paths.push(file_path.clone());
        let mut contents = vec![0u8; FILE_SIZE];
        rand::fill(&mut contents[..]);
        hash.push(blake3::hash(&contents));
        let mut file = fs::File::create_new(file_path)?;
        file.write_all(&contents)?;
    }
    for (path, hash) in file_paths.iter().zip(hash.iter()) {
        let contents = fs::read(path)?;
        let read_hash = blake3::hash(&contents);
        assert_eq!(*hash, read_hash, "ファイルの内容が一致しない: {:?}", path);
    }

    fs::remove_dir_all(&dir_path)?;
    Ok(())
}

/// [13] マルチスレッドでのファイル操作
/// (同一ディレクトリ内で操作はするが、同一ファイルを複数スレッドで同時に操作することはしない)
#[test]
#[ignore]
fn test_multithreaded_file_operations() -> Result<(), Box<dyn std::error::Error>> {
    const THREAD_COUNT: usize = 10;
    let root_dir = get_test_root_dir()?;
    let branch_num = uuid::Uuid::now_v7().to_string();
    let base_dir = root_dir.join(format!("test_13_dir_{}", branch_num));
    fs::create_dir(&base_dir)?;
    let mut join_handles = Vec::new();
    let barrier = Arc::new(Barrier::new(THREAD_COUNT));
    for _i in 0..THREAD_COUNT {
        let join = std::thread::spawn({
            let path = base_dir.clone();
            let barrier = barrier.clone();
            || thread_func(path, barrier)
        });
        join_handles.push(join);
    }
    for join in join_handles {
        match join.join() {
            Ok(result) => result?,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
    fs::remove_dir_all(base_dir)?;
    Ok(())
}

fn thread_func(path: impl AsRef<Path>, barrier: Arc<Barrier>) -> std::io::Result<()> {
    barrier.wait();
    let base_path = path.as_ref();
    let branch_num = uuid::Uuid::now_v7().to_string();

    let test_file_path = base_path.join(format!("test_13_file_{}", branch_num));
    {
        let mut file = fs::File::create(&test_file_path)?;
        file.write_all(branch_num.as_bytes())?;
    }
    {
        let content = fs::read_to_string(&test_file_path)?;
        assert_eq!(
            content, branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }
    let test_dir_path = base_path.join(format!("test_13_dir_{}", branch_num));
    let moved_file_path_suffix = test_file_path.strip_prefix(base_path).unwrap();
    let moved_file_path = test_dir_path.join(moved_file_path_suffix);
    fs::create_dir(&test_dir_path)?;
    fs::rename(&test_file_path, &moved_file_path)?;
    {
        let content = fs::read_to_string(&moved_file_path)?;
        assert_eq!(
            content, branch_num,
            "書き込んだ内容と読み出した内容が一致しない。"
        );
    }
    Ok(())
}
