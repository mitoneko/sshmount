mod cmdline_opt;
mod ssh_filesystem;

use clap::Parser;
use cmdline_opt::Opt;
use dialoguer::Password;
use dns_lookup::lookup_host;
use log::debug;
use ssh2::Session;
use ssh2_config::{HostParams, SshConfig};
use std::{
    fs::File,
    io::{BufReader, Read},
    net::TcpStream,
    path::PathBuf,
    str,
};

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

/// セッションを生成する。
fn make_ssh_session(opt: &Opt) -> Result<Session, String> {
    let host_params = get_ssh_config(&opt.config_file).query(&opt.remote.host);
    let address = get_address(opt, &host_params)?;
    let username = get_username(opt, &host_params)?;
    debug!(
        "[main] 接続先情報-> ユーザー:\"{}\", ip address:{:?}",
        &username, &address
    );
    let identity_file = get_identity_file(opt, &host_params);

    let ssh = connect_ssh(address)?;
    userauth(&ssh, &username, &identity_file)?;
    Ok(ssh)
}

/// ホストのipアドレス解決
fn get_address(opt: &Opt, host_params: &HostParams) -> Result<std::net::SocketAddr, String> {
    let dns = host_params.host_name.as_deref().unwrap_or(&opt.remote.host);
    let addr = lookup_host(dns).map_err(|_| format!("接続先ホストが見つかりません。({dns})"))?;
    Ok(std::net::SocketAddr::from((addr[0], opt.port)))
}

/// ssh-configの取得と解析
/// ファイル名が指定されていない場合は"~/.ssh/config"を使用
/// configファイルのエラー及びファイルがない場合、デフォルト値を返す。
fn get_ssh_config(file_opt: &Option<PathBuf>) -> SshConfig {
    get_config_file(file_opt)
        .map(BufReader::new)
        .map_or(SshConfig::default(), |mut f| {
            SshConfig::default().parse(&mut f).unwrap_or_else(|e| {
                eprintln!("警告:configファイル内にエラー -- {e}");
                SshConfig::default()
            })
        })
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

/// ログイン名を確定し、取得する。
/// ログイン名指定の優先順位は、1. -u引数指定, 2.remote引数, 3.ssh_config指定, 4.現在のユーザー名
fn get_username(opt: &Opt, params: &HostParams) -> Result<String, String> {
    if let Some(n) = &opt.login_name {
        Ok(n.clone())
    } else if let Some(n) = &opt.remote.user {
        Ok(n.clone())
    } else if let Some(n) = &params.user {
        Ok(n.clone())
    } else if let Some(n) = users::get_current_username() {
        n.to_str()
            .ok_or_else(|| format!("ログインユーザ名不正。({n:?})"))
            .map(|s| s.to_string())
    } else {
        Err("ユーザー名が取得できませんでした。")?
    }
}

/// 秘密キーファイルのパスを取得する
fn get_identity_file(opt: &Opt, host_params: &HostParams) -> Option<PathBuf> {
    if opt.identity.is_some() {
        opt.identity.clone()
    } else {
        host_params.identity_file.as_ref().map(|p| p[0].clone())
    }
}

/// リモートのsshに接続し、セッションを生成する。
fn connect_ssh<A: std::net::ToSocketAddrs>(address: A) -> Result<Session, String> {
    let tcp = TcpStream::connect(address)
        .map_err(|e| format!("TCP/IPの接続に失敗しました -- {:?}", &e))?;
    let mut ssh = Session::new().map_err(|e| format!("sshの接続に失敗しました。 -- {:?}", &e))?;
    ssh.set_tcp_stream(tcp);
    ssh.handshake()
        .map_err(|e| format!("sshの接続に失敗しました。(2) -- {:?}", &e))?;
    Ok(ssh)
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
