mod cmdline_opt;
mod ssh_connect;
mod ssh_filesystem;

use clap::Parser;
use cmdline_opt::Opt;
use log::debug;
use ssh2::Session;
use ssh_connect::make_ssh_session;
use std::{io::Read, path::PathBuf, str};

fn main() -> Result<(), String> {
    env_logger::init();
    let opt = Opt::parse();

    let ssh = make_ssh_session(&opt)?;

    // リモートホストのトップディレクトリの生成
    let path = make_remote_path(&opt, &ssh)?;

    // マウントオプションの調整
    let mut options = vec![fuser::MountOption::FSName("sshfs".to_string())];
    options.push(fuser::MountOption::NoDev);
    options.push(fuser::MountOption::DirSync);
    options.push(fuser::MountOption::Sync);
    match opt.readonly {
        true => options.push(fuser::MountOption::RO),
        false => options.push(fuser::MountOption::RW),
    }
    match opt.no_exec {
        true => options.push(fuser::MountOption::NoExec),
        false => options.push(fuser::MountOption::Exec),
    }
    match opt.no_atime {
        true => options.push(fuser::MountOption::NoAtime),
        false => options.push(fuser::MountOption::Atime),
    }

    // ファイルシステムへのマウント実行
    let fs = ssh_filesystem::Sshfs::new(ssh, &path);
    fuser::mount2(fs, opt.mount_point, &options).unwrap();
    Ok(())
}

/// リモート接続先のpathの生成
fn make_remote_path(opt: &Opt, session: &Session) -> Result<PathBuf, String> {
    // パスの生成
    let mut path = match opt.remote.path {
        Some(ref p) => {
            if p.is_absolute() {
                p.clone()
            } else {
                let mut h = get_home_on_remote(session)?;
                h.push(p);
                h
            }
        }
        None => get_home_on_remote(session)?,
    };
    // 生成したパスが実在するかを確認する
    let sftp = session
        .sftp()
        .map_err(|e| format!("接続作業中、リモートへのsftp接続に失敗しました。-- {e}"))?;
    let file_stat = sftp
        .stat(&path)
        .map_err(|_| format!("接続先のパスが見つかりません。{:?}", &path))?;
    if !file_stat.is_dir() {
        Err("接続先のパスはディレクトリではありません。")?;
    };
    // 生成したパスがシンボリックリンクのときは、リンク先を解決する
    let file_stat = sftp.lstat(&path).unwrap();
    if file_stat.file_type().is_symlink() {
        path = sftp
            .readlink(&path)
            .map_err(|e| format!("接続先のシンボリックリンクの解決に失敗しました。-- {e}"))?;
        if !path.is_absolute() {
            let tmp = path;
            path = get_home_on_remote(session)?;
            path.push(tmp);
        };
    };

    Ok(path)
}

/// ssh接続先のカレントディレクトリを取得する
fn get_home_on_remote(session: &Session) -> Result<PathBuf, String> {
    let mut channel = session
        .channel_session()
        .map_err(|e| format!("接続作業中、sshのチャンネル構築に失敗しました。-- {e}"))?;
    channel
        .exec("pwd")
        .map_err(|e| format!("HOMEディレクトリの取得に失敗しました。-- {e}"))?;
    let mut buf = Vec::<u8>::new();
    channel
        .read_to_end(&mut buf)
        .map_err(|e| format!("HOMEディレクトリの取得に失敗しました(2) -- {e}"))?;
    channel
        .close()
        .map_err(|e| format!("接続作業中、sshチャンネルのクローズに失敗しました。-- {e}",))?;
    str::from_utf8(&buf)
        .map_err(|e| format!("HOMEディレクトリの取得に失敗しました(3) -- {e}"))?
        .trim()
        .parse::<PathBuf>()
        .map_err(|e| format!("HOMEディレクトリの取得に失敗しました(4) -- {e}"))
}
