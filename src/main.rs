mod ssh_filesystem;

use clap::Parser;
use dns_lookup::lookup_host;
use log::debug;
use ssh2::Session;
use std::net::TcpStream;

fn main() {
    let opt = Opt::parse();
    env_logger::init();

    println!("\"{}\"をマウントする予定", opt.remote);
    // ホスト名の最終決定には、.ssh/config の情報反映が必要
    let address = lookup_host(&opt.remote.host).unwrap();
    if address.is_empty() {
        panic!("not found");
    }
    let socketaddr = std::net::SocketAddr::from((address[0], 22));
    debug!("接続先: {:?}", socketaddr);
    let tcp = TcpStream::connect(socketaddr).unwrap();
    let mut ssh = Session::new().unwrap();
    ssh.set_tcp_stream(tcp);
    ssh.handshake().unwrap();
    // 認証情報は、今は固定
    let key = std::path::Path::new("/home/mito/.ssh/id_rsa");
    ssh.userauth_pubkey_file("mito", None, key, None).unwrap();
    let fs = ssh_filesystem::Sshfs::new(ssh);

    let options = vec![fuser::MountOption::FSName("sshfs".to_string())];
    fuser::mount2(fs, opt.mount_point, &options).unwrap();
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
                h.trim().to_string(),
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
    }
}
