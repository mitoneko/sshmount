//! ssh接続関連関数モジュール

use crate::cmdline_opt::Opt;
use anyhow::{anyhow, Context, Result};
use dialoguer::Password;
use dns_lookup::lookup_host;
use log::debug;
use ssh2::Session;
use ssh2_config::{HostParams, ParseRule, SshConfig};
use std::{
    fs::File,
    io::BufReader,
    net::TcpStream,
    path::{Path, PathBuf},
    str,
};

/// セッションを生成する。
pub fn make_ssh_session(opt: &Opt) -> Result<Session> {
    let host_params = get_ssh_config(&opt.config_file).query(&opt.remote.host);
    let address = get_address(opt, &host_params).context("Failed to get host address")?;
    let username = get_username(opt, &host_params).context("Failed to get user name.")?;
    debug!(
        "[main] 接続先情報-> ユーザー:\"{}\", ip address:{:?}",
        &username, &address
    );
    let identity_file = get_identity_file(opt, &host_params)?;

    let ssh = connect_ssh(address).context("The ssh connection failed.")?;
    userauth(&ssh, &username, &identity_file).context("User authentication failed.")?;
    Ok(ssh)
}

/// ホストのipアドレス解決
fn get_address(opt: &Opt, host_params: &HostParams) -> Result<std::net::SocketAddr> {
    let dns = host_params.host_name.as_deref().unwrap_or(&opt.remote.host);
    let addr = lookup_host(dns).context("Cannot find host to connect to.")?;
    Ok(std::net::SocketAddr::from((addr[0], opt.port)))
}

/// ssh-configの取得と解析
/// ファイル名が指定されていない場合は"~/.ssh/config"を使用
/// configファイルのエラー及びファイルがない場合、デフォルト値を返す。
fn get_ssh_config(file_opt: &Option<PathBuf>) -> SshConfig {
    get_config_file(file_opt)
        .map(BufReader::new)
        .map_or(SshConfig::default(), |mut f| {
            SshConfig::default()
                .parse(&mut f, ParseRule::ALLOW_UNKNOWN_FIELDS)
                .unwrap_or_else(|e| {
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
fn get_username(opt: &Opt, params: &HostParams) -> Result<String> {
    if let Some(n) = &opt.login_name {
        Ok(n.clone())
    } else if let Some(n) = &opt.remote.user {
        Ok(n.clone())
    } else if let Some(n) = &params.user {
        Ok(n.clone())
    } else if let Some(n) = users::get_current_username() {
        n.to_str()
            .map(|s| s.to_string())
            .ok_or(anyhow!("Invalid login user name. -- {n:?}"))
    } else {
        Err(anyhow!("Could not obtain user name."))
    }
}

/// 秘密キーファイルのパスを取得する
fn get_identity_file(opt: &Opt, host_params: &HostParams) -> Result<Option<PathBuf>> {
    if let Some(n) = &opt.identity {
        std::fs::File::open(n).with_context(|| {
            format!(
                "Unable to access the secret key file specified by the \"-i\" option. [{:?}]",
                &n
            )
        })?;
        Ok(Some(n.clone()))
    } else {
        let name = host_params.identity_file.as_ref().map(|p| p[0].clone());
        if let Some(ref n) = name {
            std::fs::File::open(n).with_context(|| {
                format!(
                    "Unnable to access the secret file specified by the ssh-config. [{:?}]",
                    &n
                )
            })?;
        }
        Ok(name)
    }
}

/// リモートのsshに接続し、セッションを生成する。
fn connect_ssh<A: std::net::ToSocketAddrs>(address: A) -> Result<Session> {
    let tcp = TcpStream::connect(address).context("Failed to connect to TCP/IP.")?;
    let mut ssh = Session::new().context("Failed to connect to ssh.")?;
    ssh.set_tcp_stream(tcp);
    ssh.handshake().context("Failed to hanshake ssh.")?;
    Ok(ssh)
}

/// ssh認証を実施する。
fn userauth(sess: &Session, username: &str, identity: &Option<PathBuf>) -> Result<()> {
    if user_auth_agent(sess, username).is_ok() {
        return Ok(());
    }
    if let Some(f) = identity {
        if user_auth_identity(sess, username, f).is_ok() {
            return Ok(());
        }
    }
    user_auth_password(sess, username)
        .map_err(|_| anyhow!("All user authentication methods failed."))
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
                .with_prompt("Enter the passphrase for the secret key.")
                .allow_empty_password(true)
                .interact()
                .map_err(|e| e.to_string())?;
            ret = sess.userauth_pubkey_file(username, None, key_file, Some(&password));
            if ret.is_ok() {
                return Ok(());
            }
            eprintln!("The passphrase is different.");
        }
    }
    debug!("認証失敗(pubkey)->{:?}", ret.as_ref().unwrap_err());
    Err("公開キー認証失敗".to_string())
}

/// パスワード認証
fn user_auth_password(sess: &Session, username: &str) -> Result<(), String> {
    for _i in 0..3 {
        let password = Password::new()
            .with_prompt("Enter your login password.")
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
        eprintln!("The password is different.");
        debug!("認証失敗(password)->{:?}", ret.unwrap_err());
    }
    Err("パスワード認証失敗".to_string())
}
