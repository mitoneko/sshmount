mod ssh_filesystem;

use clap::Parser;
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

    let fs = ssh_filesystem::Sshfs::new(ssh, &path);
    let options = vec![fuser::MountOption::FSName("sshfs".to_string())];
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

/// コマンドラインオプション
#[derive(Parser)]
#[command(author, version, about)]
struct Opt {
    /// 接続先 [user@]host:[path]
    remote: RemoteName,
    /// マウント先のパス
    #[arg(value_parser = exist_dir)]
    mount_point: String,
    /// configファイルのパス指定
    #[arg(short = 'F', long)]
    config_file: Option<PathBuf>,
    /// ログイン名
    #[arg(short, long)]
    login_name: Option<String>,
    /// 秘密キーファイル名
    #[arg(short, long)]
    identity: Option<PathBuf>,
    /// ポート番号
    #[arg(short, long, default_value_t = 22)]
    port: u16,
}

/// 指定されたディレクトリが存在し、中にファイルがないことを確認する。
fn exist_dir(s: &str) -> Result<String, String> {
    match std::fs::read_dir(s) {
        Ok(mut dir) => match dir.next() {
            None => Ok(s.to_string()),
            Some(_) => Err("マウント先ディレクトリが空ではありません".to_string()),
        },
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                Err("マウント先ディレクトリが存在しません。".to_string())
            }
            _ => Err("計り知れないエラーです。".to_string()),
        },
    }
}

/// コマンドラインの接続先ホスト情報
#[derive(Clone, Debug, PartialEq)]
struct RemoteName {
    /// ユーザー名
    user: Option<String>,
    /// ホスト名　または　IPアドレス
    host: String,
    /// 接続先パス
    path: Option<std::path::PathBuf>,
}

impl std::fmt::Display for RemoteName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = format!("<{:?}><{:?}><{:?}>", &self.user, &self.host, &self.path);
        s.fmt(f)
    }
}

impl std::str::FromStr for RemoteName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut rest_str = s;
        let user = match rest_str.split_once('@') {
            Some((u, r)) => {
                rest_str = r;
                if !u.trim().is_empty() {
                    Some(u.trim().to_string())
                } else {
                    None
                }
            }
            None => None,
        };
        let (host, path) = match rest_str.split_once(':') {
            Some((h, p)) => (
                if !h.trim().is_empty() {
                    h.trim().to_string()
                } else {
                    return Err("接続先ホストの形式は、\"[user@]host:[path]\"です。".to_string());
                },
                if !p.trim().is_empty() {
                    Some(std::path::PathBuf::from(p.trim().to_string()))
                } else {
                    None
                },
            ),
            None => return Err("接続先ホストの形式は、\"[user@]host:[path]\"です。".to_string()),
        };
        Ok(Self { user, host, path })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Opt::command().debug_assert()
    }

    #[test]
    fn test_from_str_remotename() {
        use std::path::Path;
        let s = "mito@reterminal.local:/home/mito";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: "reterminal.local".to_string(),
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "mito@reterminal.local:/home/mito/";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: "reterminal.local".to_string(),
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "reterminal.local:";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: None,
            host: "reterminal.local".to_string(),
            path: None,
        };
        assert_eq!(r, k);

        let s = " mito @reterminal.local: ";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: "reterminal.local".to_string(),
            path: None,
        };
        assert_eq!(r, k);

        let s = "reterminal.local";
        let r: Result<RemoteName, String> = s.parse();
        assert_eq!(
            r,
            Err("接続先ホストの形式は、\"[user@]host:[path]\"です。".to_string())
        );

        let s = "mito@reterminal.local";
        let r: Result<RemoteName, String> = s.parse();
        assert_eq!(
            r,
            Err("接続先ホストの形式は、\"[user@]host:[path]\"です。".to_string())
        );

        let s = " mito @: ";
        let r: Result<RemoteName, String> = s.parse();
        assert_eq!(
            r,
            Err("接続先ホストの形式は、\"[user@]host:[path]\"です。".to_string())
        );
    }
}
