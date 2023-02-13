mod cmdline_opt;
mod ssh_filesystem;

use clap::Parser;
use cmdline_opt::Opt;
use dialoguer::Password;
use dns_lookup::lookup_host;
use log::debug;
use ssh2::Session;
use ssh2_config::SshConfig;
use std::{
    fs::File,
    io::{BufReader, Read},
    net::TcpStream,
    path::PathBuf,
    str,
};

fn main() -> Result<(), String> {
    let opt = Opt::parse();
    env_logger::init();

    // ssh configファイルの取得と解析
    let mut ssh_config = SshConfig::default();
    {
        let file = get_config_file(&opt.config_file).map(BufReader::new);
        if let Some(mut f) = file {
            match SshConfig::default().parse(&mut f) {
                Ok(c) => ssh_config = c,
                Err(e) => eprintln!("警告:configファイル内にエラー -- {e}"),
            };
        };
    }

    // ssh configのエイリアスを解決し、接続アドレスの逆引き。
    let mut dns = &opt.remote.host;
    let host_params = ssh_config.query(dns);
    if let Some(ref n) = host_params.host_name {
        dns = n
    };
    let address = lookup_host(dns).map_err(|_| format!("接続先ホストが見つかりません。({dns})"))?;

    // ログイン名の確定
    let username: String = if let Some(n) = &opt.login_name {
        n.clone()
    } else if let Some(n) = &opt.remote.user {
        n.clone()
    } else if let Some(n) = &host_params.user {
        n.clone()
    } else if let Some(n) = users::get_current_username() {
        n.to_str()
            .ok_or_else(|| format!("ログインユーザ名不正。({n:?})"))?
            .to_string()
    } else {
        Err("ユーザー名が取得できませんでした。")?
    };
    debug!("[main] ログインユーザー名: {}", &username);

    // 秘密キーファイル名の取得
    let identity_file: Option<PathBuf> = if let Some(ref i) = host_params.identity_file {
        Some(i[0].clone())
    } else {
        opt.identity.as_ref().cloned()
    };

    // ssh接続作業
    let socketaddr = std::net::SocketAddr::from((address[0], opt.port));
    debug!("接続先: {:?}", socketaddr);
    let tcp = TcpStream::connect(socketaddr).unwrap();
    let mut ssh = Session::new().unwrap();
    ssh.set_tcp_stream(tcp);
    ssh.handshake().unwrap();
    // ssh認証
    userauth(&ssh, &username, &identity_file)?;

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

/// ssh認証を実施する。
fn userauth(sess: &Session, username: &str, identity: &Option<PathBuf>) -> Result<(), String> {
    let ret = sess.userauth_agent(username);
    if ret.is_ok() {
        return Ok(());
    }
    debug!("認証失敗(agent)->{:?}", ret.unwrap_err());
    if let Some(f) = identity {
        let ret = sess.userauth_pubkey_file(username, None, f, None);
        if ret.is_ok() {
            return Ok(());
        }
        if let ssh2::ErrorCode::Session(-16) = ret.as_ref().unwrap_err().code() {
            // error_code -16 ->
            // LIBSSH2_ERROR_FILE:PUBLIC_KEYの取得失敗。多分、秘密キーのパスフレーズ
            for _i in 0..3 {
                let password = Password::new()
                    .with_prompt("秘密キーのパスフレーズを入力してください。")
                    .allow_empty_password(true)
                    .interact()
                    .map_err(|e| e.to_string())?;
                let ret = sess.userauth_pubkey_file(username, None, f, Some(&password));
                if ret.is_ok() {
                    return Ok(());
                }
                eprintln!("パスフレーズが違います。");
            }
        }
        debug!("認証失敗(pubkey)->{:?}", ret.unwrap_err());
    }
    for _i in 0..3 {
        let password = Password::new()
            .with_prompt("ログインパスワードを入力してください。")
            .allow_empty_password(true)
            .interact()
            .map_err(|e| e.to_string())?;
        let ret = sess.userauth_password(username, &password);
        if ret.is_ok() {
            return Ok(());
        }
        let ssh2::ErrorCode::Session(-18) = ret.as_ref().unwrap_err().code() else { break; };
        // ssh2エラーコード　-18 ->
        // LIBSSH2_ERROR_AUTHENTICATION_FAILED: パスワードが違うんでしょう。
        eprintln!("パスワードが違います。");
        debug!("認証失敗(password)->{:?}", ret.unwrap_err());
    }
    Err("sshの認証に失敗しました。".to_string())
}

/// ssh_configファイルがあれば、オープンする。
/// ファイル名の指定がなければ、$Home/.ssh/configを想定する。
fn get_config_file(file_name: &Option<PathBuf>) -> Option<std::fs::File> {
    let file_name = file_name.clone().or_else(|| {
        home::home_dir().map(|p| {
            let mut p = p;
            p.push(".ssh/config");
            p
        })
    });

    file_name.and_then(|p| File::open(p).ok())
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
