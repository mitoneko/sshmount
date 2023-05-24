//! FUSEパラメータ関係　ユーティリティ

use crate::cmdline_opt::Opt;
use anyhow::{ensure, Context, Result};
use ssh2::Session;
use std::env::current_dir;
use std::{
    io::Read,
    path::{Path, PathBuf},
    str,
};

/// マウントポイントのフルパスを生成する
pub fn make_full_path<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    if path.as_ref().is_absolute() {
        Ok(path.as_ref().to_path_buf())
    } else {
        let mut full_path = current_dir().context("cannot access current directory.")?;
        full_path.push(path);
        Ok(full_path)
    }
}

/// リモート接続先のpathの生成
pub fn make_remote_path(opt: &Opt, session: &Session) -> Result<PathBuf> {
    // パスの生成
    const MSG_ERRORHOME: &str = "Fail to generate path name.";
    let mut path = match opt.remote.path {
        Some(ref p) => {
            if p.is_absolute() {
                p.clone()
            } else {
                let mut h = get_home_on_remote(session).context(MSG_ERRORHOME)?;
                h.push(p);
                h
            }
        }
        None => get_home_on_remote(session).context(MSG_ERRORHOME)?,
    };
    // 生成したパスが実在するかを確認する
    let sftp = session
        .sftp()
        .context("Connection to SFTP failed when checking for existence of a path.")?;
    let file_stat = sftp
        .stat(&path)
        .with_context(|| format!("Cannot find path to connect to. path={:?}", &path))?;
    ensure!(
        file_stat.is_dir(),
        "The path to connect to is not a directory."
    );
    // 生成したパスがシンボリックリンクのときは、リンク先を解決する
    let file_stat = sftp
        .lstat(&path)
        .context("Failed to obtain the attributes of the destination directory.")?;
    if file_stat.file_type().is_symlink() {
        path = sftp
            .readlink(&path)
            .context("Failed to resolve symbolic link to connect to.")?;
        if !path.is_absolute() {
            let tmp = path;
            path = get_home_on_remote(session)
                .context("Failed to complete the symbolic link to connect to.")?;
            path.push(tmp);
        };
    };

    Ok(path)
}

/// FUSEの接続時オプションを生成する
pub fn make_mount_option(cmd_opt: &Opt) -> Vec<fuser::MountOption> {
    use fuser::MountOption;

    let mut options = vec![MountOption::FSName("sshfs".to_string())];
    options.push(MountOption::NoDev);
    options.push(MountOption::DirSync);
    options.push(MountOption::Sync);
    match cmd_opt.readonly {
        true => options.push(MountOption::RO),
        false => options.push(MountOption::RW),
    }
    match cmd_opt.no_exec {
        true => options.push(MountOption::NoExec),
        false => options.push(MountOption::Exec),
    }
    match cmd_opt.no_atime {
        true => options.push(MountOption::NoAtime),
        false => options.push(MountOption::Atime),
    }
    options
}

/// ssh接続先のカレントディレクトリを取得する
fn get_home_on_remote(session: &Session) -> Result<PathBuf> {
    let mut channel = session
        .channel_session()
        .context("Fail to build ssh channel.")?;
    channel
        .exec("pwd")
        .context("Fail to execute \"pwd\" command.")?;
    let mut buf = Vec::<u8>::new();
    channel
        .read_to_end(&mut buf)
        .context("Fail to get response for \"pwd\" command.")?;
    channel.close().context("Fail to close ssh channel.")?;
    str::from_utf8(&buf)
        .context("The pwd result contains non-utf8 characters.")?
        .trim()
        .parse::<PathBuf>()
        .context("Fail to build path name.")
}
