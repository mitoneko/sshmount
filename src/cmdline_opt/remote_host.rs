//! コマンドラインオブションにおけるRemoteHost構造体

/// コマンドラインの接続先ホスト情報
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteName {
    /// ユーザー名
    pub user: Option<String>,
    /// ホスト名　または　IPアドレス
    pub host: String,
    /// ポート番号
    pub port: Option<u16>,
    /// 接続先パス
    pub path: Option<std::path::PathBuf>,
}

impl std::fmt::Display for RemoteName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = format!(
            "<{:?}><{:?}><{:?}><{:?}>",
            &self.user, &self.host, &self.port, &self.path
        );
        s.fmt(f)
    }
}

impl std::str::FromStr for RemoteName {
    type Err = ErrorRemoteName;
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
                    return Err(ErrorRemoteName);
                },
                if !p.trim().is_empty() {
                    Some(std::path::PathBuf::from(p.trim().to_string()))
                } else {
                    None
                },
            ),
            None => return Err(ErrorRemoteName),
        };
        Ok(Self {
            user,
            host,
            port: None,
            path,
        })
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[error("The format of the host to connect to is \"[user@]host:[path]\" or \"scp://[user@]host[:port][/path]\".")]
pub struct ErrorRemoteName;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_from_str_remotename() {
        use std::path::Path;
        let s = "mito@reterminal.local:/home/mito";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: "reterminal.local".to_string(),
            port: None,
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "mito@reterminal.local:/home/mito/";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: "reterminal.local".to_string(),
            port: None,
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "reterminal.local:";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: None,
            host: "reterminal.local".to_string(),
            port: None,
            path: None,
        };
        assert_eq!(r, k);

        let s = " mito @reterminal.local: ";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: "reterminal.local".to_string(),
            port: None,
            path: None,
        };
        assert_eq!(r, k);

        let s = "reterminal.local";
        let r: Result<RemoteName, ErrorRemoteName> = s.parse();
        assert_eq!(r, Err(ErrorRemoteName));

        let s = "mito@reterminal.local";
        let r: Result<RemoteName, ErrorRemoteName> = s.parse();
        assert_eq!(r, Err(ErrorRemoteName));

        let s = " mito @: ";
        let r: Result<RemoteName, ErrorRemoteName> = s.parse();
        assert_eq!(r, Err(ErrorRemoteName));
    }
}
