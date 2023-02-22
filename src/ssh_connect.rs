//! ssh接続関連関数モジュール

use crate::cmdline_opt::Opt;
use dialoguer::Password;
use dns_lookup::lookup_host;
use log::debug;
use ssh2::Session;
use ssh2_config::{HostParams, SshConfig};
use std::{
    fs::File,
    io::BufReader,
    net::TcpStream,
    path::{Path, PathBuf},
    str,
};

/// セッションを生成する。
pub fn make_ssh_session(opt: &Opt) -> Result<Session, String> {
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
    if user_auth_agent(sess, username).is_ok() {
        return Ok(());
    }
    if let Some(f) = identity {
        if user_auth_identity(sess, username, f).is_ok() {
            return Ok(());
        }
    }
    user_auth_password(sess, username)
}

/// agent認証
fn user_auth_agent(sess: &Session, username: &str) -> Result<(), ssh2::Error> {
    let ret = sess.userauth_agent(username);
    if ret.is_err() {
        debug!("認証失敗(agent)->{:?}", ret.as_ref().unwrap_err());
    };
    ret
}

/// 公開キー認証
fn user_auth_identity(sess: &Session, username: &str, key_file: &Path) -> Result<(), String> {
    let mut ret = sess.userauth_pubkey_file(username, None, key_file, None);
    if ret.is_ok() {
        return Ok(());
    };
    if let ssh2::ErrorCode::Session(-16) = ret.as_ref().unwrap_err().code() {
        // error_code -16 ->
        // LIBSSH2_ERROR_FILE:PUBLIC_KEYの取得失敗。多分、秘密キーのパスフレーズ
        for _i in 0..3 {
            let password = Password::new()
                .with_prompt("秘密キーのパスフレーズを入力してください。")
                .allow_empty_password(true)
                .interact()
                .map_err(|e| e.to_string())?;
            ret = sess.userauth_pubkey_file(username, None, key_file, Some(&password));
            if ret.is_ok() {
                return Ok(());
            }
            eprintln!("パスフレーズが違います。");
        }
    }
    debug!("認証失敗(pubkey)->{:?}", ret.as_ref().unwrap_err());
    Err("公開キー認証失敗".to_string())
}

/// パスワード認証
fn user_auth_password(sess: &Session, username: &str) -> Result<(), String> {
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
    Err("パスワード認証失敗".to_string())
}
