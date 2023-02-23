use clap::Parser;
use std::path::PathBuf;

/// コマンドラインオプション
#[derive(Parser)]
#[command(author, version, about)]
pub struct Opt {
    /// 接続先 [user@]host:[path]
    pub remote: RemoteName,
    /// マウント先のパス
    #[arg(value_parser = exist_dir)]
    pub mount_point: String,
    /// configファイルのパス指定
    #[arg(short = 'F', long)]
    pub config_file: Option<PathBuf>,
    /// ログイン名
    #[arg(short, long)]
    pub login_name: Option<String>,
    /// 秘密キーファイル名
    #[arg(short, long)]
    pub identity: Option<PathBuf>,
    /// ポート番号
    #[arg(short, long, default_value_t = 22)]
    pub port: u16,
    /// リードオンリー
    #[arg(short, long)]
    pub readonly: bool,
    /// 実行不可
    #[arg(long)]
    pub no_exec: bool,
    /// アクセス日時(atime)を変更しない。
    #[arg(long)]
    pub no_atime: bool,
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
            std::io::ErrorKind::NotConnected => {
                Err("マウント先ディレクトリのネットワークが切断されています。(umountを忘れていませんか?)".to_string())
            }
            _ => Err(format!("計り知れないエラーです。--{e:?}")),
        },
    }
}

/// コマンドラインの接続先ホスト情報
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteName {
    /// ユーザー名
    pub user: Option<String>,
    /// ホスト名　または　IPアドレス
    pub host: String,
    /// 接続先パス
    pub path: Option<std::path::PathBuf>,
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
