//! ssh接続関連関数モジュール

use crate::cmdline_opt::Opt;
use anyhow::{anyhow, Context, Result};
use dialoguer::Password;
use dns_lookup::lookup_host;
use log::{debug, error, info, warn};
use ssh2::Session;
use ssh2_config::{HostParams, ParseRule, SshConfig};
use std::{
    fs::File,
    io::BufReader,
    net::TcpStream,
    path::{Path, PathBuf},
    str,
};

/// ssh認証で試行するキーファイルの最大数。
/// あまりに、トライ数が多いとサーバー負荷等に影響があるかもしれないので制限する。
const MAX_IDENTITY_TRY: usize = 10;
/// デフォルトのポート番号
const DEFAULT_PORT: u16 = 22;

/// セッションを生成する。
pub fn make_ssh_session(opt: &Opt) -> Result<Session> {
    let host_params = make_host_params(opt).context("Failed to make host parameters.")?;
    let addresses = get_address(&host_params)?;
    let user_name = host_params
        .user
        .as_ref()
        .ok_or(anyhow!("User name is not specified."))?;
    info!(
        "[main] info connection-> user name:\"{}\", ip address:{:?}",
        &user_name, &addresses
    );
    let identity_file = &host_params.identity_file;

    let ssh = connect_ssh(&addresses[..]).context("The ssh connection failed.")?;
    userauth(&ssh, user_name, identity_file).context("User authentication failed.")?;
    info!("success connect ssh: ip=>{:?}", addresses);
    Ok(ssh)
}

/// ホストパラメータの生成
/// configファイルより取得したホスト情報をもとに、コマンドラインオプションで上書きしたホストパラメータを生成する。
/// ホスト情報は、コマンドラインオプション>configファイル>remote_host引数の順で上書きする。
fn make_host_params(opt: &Opt) -> Result<HostParams> {
    let mut host_params = get_ssh_config(&opt.config_file).query(opt.remote.host.to_string());
    // ホスト名の解決
    if host_params.host_name.is_none() {
        host_params.host_name = Some(opt.remote.host.to_string());
    }
    // ユーザー名の解決
    host_params.user = Some(get_username(opt, &host_params).context("Failed to get user name.")?);
    // 秘密キーファイルの解決
    host_params.identity_file = get_identity_file(opt, &host_params)?;
    // ポート番号の解決
    host_params.port = Some(
        opt.port.unwrap_or(
            host_params
                .port
                .unwrap_or(opt.remote.port.unwrap_or(DEFAULT_PORT)),
        ),
    );
    Ok(host_params)
}

/// ホストのipアドレス解決
fn get_address(host_params: &HostParams) -> Result<Vec<std::net::SocketAddr>> {
    let dns = host_params
        .host_name
        .as_ref()
        .ok_or(anyhow!("Host name is not specified."))?;
    let port = host_params
        .port
        .ok_or(anyhow!("Port number is not specified."))?;
    let addrs = lookup_host(dns)
        .inspect_err(|e| error!("get_address : Failed lookup_host[{}]", e))
        .context("Cannot find host to connect to.")?
        .map(|addr| std::net::SocketAddr::from((addr, port)))
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(anyhow!("No address found for the specified host."));
    }
    Ok(addrs)
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
                    eprintln!("Warning: Failed to parse ssh_config file. Using default settings. (error: {})", e);
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
fn get_identity_file(opt: &Opt, host_params: &HostParams) -> Result<Option<Vec<PathBuf>>> {
    if let Some(n) = &opt.identity {
        let path = expand_tilde_in_path(n);
        std::fs::File::open(&path).with_context(|| {
            format!(
                "Unable to access the secret key file specified by the \"-i\" option. [{}]",
                &path.to_string_lossy()
            )
        })?;
        Ok(Some(vec![path]))
    } else {
        let name = host_params.identity_file.as_ref();
        match name {
            Some(n) => {
                let paths = n
                    .iter()
                    .map(expand_tilde_in_path)
                    .filter(|p|  match std::fs::File::open(p) {
                        Ok(_) => true,
                        Err(e) => {
                            warn!(
                                "IdentityFile '{:?}' from ssh-config is not accessible. skipping. (io error: {})",
                                p, e
                            );
                            eprintln!(
                                "Warning: IdentityFile '{:?}' from ssh-config is not accessible. skipping.",
                                p
                            );
                            false
                        }
                    })
                    .collect::<Vec<_>>();
                if paths.is_empty() {
                    Err(anyhow!(
                        "No usable identity files found for host {:?} (checked {} entries from ssh-config).",
                        host_params.host_name.as_deref().unwrap_or("<unknown>"),
                        n.len()
                    ))
                } else {
                    Ok(Some(paths))
                }
            }
            None => Ok(None),
        }
    }
}

// ファイル名の~記号を展開する。
fn expand_tilde_in_path(path: impl AsRef<Path>) -> PathBuf {
    let path_str = path.as_ref().to_string_lossy();
    let expanded_path = shellexpand::tilde(&path_str);
    PathBuf::from(expanded_path.as_ref())
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
fn userauth(sess: &Session, username: &str, identity: &Option<Vec<PathBuf>>) -> Result<()> {
    if user_auth_agent(sess, username).is_ok() {
        return Ok(());
    }
    if let Some(f) = identity {
        let ret = f
            .iter()
            .take(MAX_IDENTITY_TRY)
            .any(|f| user_auth_identity(sess, username, f).is_ok());
        if ret {
            return Ok(());
        }
    }
    user_auth_password(sess, username)
        .map_err(|_| anyhow!("All user authentication methods failed."))
}

/// agent認証
fn user_auth_agent(sess: &Session, username: &str) -> Result<(), ssh2::Error> {
    let ret = sess.userauth_agent(username);
    if let Err(e) = &ret {
        debug!("認証失敗(agent)->{:?}", e);
    };
    ret
}

/// 公開キー認証
fn user_auth_identity(sess: &Session, username: &str, key_file: &Path) -> Result<()> {
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
                .interact()?;
            ret = sess.userauth_pubkey_file(username, None, key_file, Some(&password));
            if ret.is_ok() {
                return Ok(());
            }
            eprintln!("The passphrase is different.");
        }
    }
    debug!(
        "Authentication failed(pubkey)->{:?}",
        ret.as_ref().unwrap_err()
    );
    Err(anyhow!("Public key authentication failed."))
}

/// パスワード認証
fn user_auth_password(sess: &Session, username: &str) -> Result<()> {
    for _i in 0..3 {
        let password = Password::new()
            .with_prompt("Enter your login password.")
            .allow_empty_password(true)
            .interact()?;
        let ret = sess.userauth_password(username, &password);
        if ret.is_ok() {
            return Ok(());
        }
        let ssh2::ErrorCode::Session(-18) = ret.as_ref().unwrap_err().code() else {
            break;
        };
        // ssh2エラーコード　-18 ->
        // LIBSSH2_ERROR_AUTHENTICATION_FAILED: パスワードが違うんでしょう。
        eprintln!("The password is different.");
        debug!("Authentication failed(password)->{:?}", ret.unwrap_err());
    }
    Err(anyhow!("Password authentication failed."))
}

#[cfg(test)]
mod test {
    use super::*;
    use clap::Parser;
    #[test]
    #[ignore]
    fn make_host_params_test() {
        let config_file_path = test_config_file_path();
        let identify = make_dummyidentity_file(1);
        let opt = make_dummy_opt(format!(
            "sshmount -F {} -i {} -p 2223 test_host:/remote/path mnt",
            config_file_path.to_string_lossy(),
            identify.to_string_lossy()
        ));
        let host_param = make_host_params(&opt).unwrap();
        assert_eq!(host_param.host_name.unwrap(), "example.com");
        assert_eq!(host_param.port.unwrap(), 2223);
        assert_eq!(host_param.user.unwrap(), "testuser");
    }

    #[test]
    #[ignore]
    fn test_make_host_params_default_port() {
        let config_file_path = test_config_file_path();
        let opt = make_dummy_opt(format!(
            "sshmount -F {} default_port:/remote/path mnt",
            config_file_path.to_string_lossy(),
        ));
        let host_param = make_host_params(&opt).unwrap();
        assert_eq!(host_param.host_name.unwrap(), "default.example.com");
        assert_eq!(host_param.port.unwrap(), DEFAULT_PORT);
        assert_eq!(host_param.user.unwrap(), "defaultuser");
    }

    #[test]
    #[ignore]
    fn test_make_host_params_ip_address_config() {
        let config_file_path = test_config_file_path();
        let opt = make_dummy_opt(format!(
            "sshmount -F {} 192.168.0.100:/remote/path mnt",
            config_file_path.to_string_lossy(),
        ));
        let host_param = make_host_params(&opt).unwrap();
        assert_eq!(host_param.host_name.unwrap(), "192.168.0.101");
        assert_eq!(host_param.port.unwrap(), 2200);
        assert_eq!(host_param.user.unwrap(), "admin");
        assert_eq!(
            host_param.identity_file.unwrap()[0],
            PathBuf::from("/home/mito/develop/rust/sshmount/test_data/dummy2_rsa")
        );
    }

    #[test]
    #[ignore]
    fn test_make_host_params_multi_identify() {
        let config_file_path = test_config_file_path();
        let opt = make_dummy_opt(format!(
            "sshmount -F {} multi_identity:/remote/path mnt",
            config_file_path.to_string_lossy(),
        ));
        let host_param = make_host_params(&opt).unwrap();
        assert_eq!(host_param.host_name.unwrap(), "multi.example.com");
        assert_eq!(
            host_param.identity_file.as_ref().unwrap()[0],
            PathBuf::from("/home/mito/develop/rust/sshmount/test_data/dummy1_rsa")
        );
        assert_eq!(
            host_param.identity_file.as_ref().unwrap()[1],
            PathBuf::from("/home/mito/develop/rust/sshmount/test_data/dummy2_rsa")
        );
        assert_eq!(host_param.identity_file.as_ref().unwrap().len(), 2);
    }

    fn test_config_file_path() -> PathBuf {
        let d = env!("CARGO_MANIFEST_DIR");
        let mut p = PathBuf::new();
        p.push(d);
        p.push("test_data/config");
        p
    }

    fn make_dummyidentity_file(no: u16) -> PathBuf {
        let d = env!("CARGO_MANIFEST_DIR");
        let mut p = PathBuf::new();
        p.push(d);
        p.push(format!("test_data/dummy{}_rsa", no));
        p
    }

    fn make_dummy_opt(cmdline: impl AsRef<str>) -> Opt {
        let args = cmdline.as_ref().split_whitespace();
        Opt::try_parse_from(args).unwrap()
    }
}
