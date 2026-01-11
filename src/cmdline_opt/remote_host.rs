//! コマンドラインオブションにおけるRemoteHost構造体

use log::debug;
use std::net::IpAddr;

/// コマンドラインの接続先ホスト情報
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteName {
    /// ユーザー名
    pub user: Option<String>,
    /// IPアドレス
    pub host: HostInfo,
    /// ポート番号
    pub port: Option<u16>,
    /// 接続先パス
    pub path: Option<std::path::PathBuf>,
}

/// ホスト情報(ホスト名または、IPアドレス)
#[derive(Clone, Debug, PartialEq)]
pub enum HostInfo {
    Name(String),
    Ip(IpAddr),
}

impl RemoteName {
    /// remote引数の解析(URI形式の場合)
    fn parse_uri(s: &str) -> Result<Self, ErrorRemoteName> {
        // URIの解析
        let uri = fluent_uri::Uri::parse(s).map_err(|_| ErrorRemoteName)?;
        let info = uri.authority().ok_or(ErrorRemoteName)?;
        // ポート番号の取得
        let port = info.port_to_u16().map_err(|_| ErrorRemoteName)?;
        // ホストアドレスの取得
        let host = match info.host_parsed() {
            fluent_uri::component::Host::Ipv4(ip) => HostInfo::Ip(ip.into()),
            fluent_uri::component::Host::Ipv6(ip) => HostInfo::Ip(ip.into()),
            _ => HostInfo::Name(info.host().to_string()),
        };
        // パス名の取得
        let path_str = uri.path().as_str();
        let path = if path_str.is_empty() {
            None
        } else {
            Some(
                path_str
                    .parse::<std::path::PathBuf>()
                    .map_err(|_| ErrorRemoteName)?,
            )
        };
        // ユーザー名の取得
        let user = info.userinfo().map(|s| s.as_str().to_string());

        Ok(Self {
            user,
            host,
            port,
            path,
        })
    }

    /// remote引数の解析(URI形式でない場合)
    fn parse_non_uri(s: &str) -> Result<Self, ErrorRemoteName> {
        let mut rest_str = s;
        // ユーザー名の取得
        let user = match rest_str.split_once("@") {
            Some((u, r)) => {
                rest_str = r;
                Some(u.trim().to_string())
            }
            None => None,
        };
        // ホストアドレスの取得
        let mut host = None;
        // IP6vの判定
        let ip6_open = rest_str.trim_start().find("[");
        let ip6_close = rest_str.find("]");
        match ip6_open {
            Some(0) => {
                if ip6_close.is_some() {
                    rest_str = rest_str.split_once("[").unwrap().1;
                    let (ip6_str, rest) = rest_str.split_once("]").unwrap();
                    host = Some(ip6_str);
                    rest_str = rest;
                    (_, rest_str) = rest_str.split_once(":").ok_or(ErrorRemoteName)?;
                // 後続の:がない。
                } else {
                    return Err(ErrorRemoteName); // 閉じカッコがない。
                }
            }
            Some(_) => {}
            None => {}
        }

        // hostがNoneなら、IP4vかホスト名
        if host.is_none() {
            match rest_str.split_once(":") {
                Some((h, r)) => {
                    if h.trim().is_empty() {
                        // ホスト名が空では困る。
                        return Err(ErrorRemoteName);
                    }
                    host = Some(h);
                    rest_str = r;
                }
                None => {
                    return Err(ErrorRemoteName);
                }
            }
        }

        // この時点で、hostがNoneであることは有り得ない。
        let try_ip = host.unwrap().parse::<IpAddr>();
        let host = match try_ip {
            Ok(addr) => HostInfo::Ip(addr),
            Err(_) => HostInfo::Name(host.unwrap().to_string()),
        };

        // パス名の取得
        let path = if rest_str.trim_end().is_empty() {
            None
        } else {
            Some(
                rest_str
                    .trim_end()
                    .parse::<std::path::PathBuf>()
                    .map_err(|_| ErrorRemoteName)?,
            )
        };

        Ok(Self {
            host,
            port: None,
            user,
            path,
        })
    }
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
        let ret = if s.trim_start().starts_with("scp://") {
            Self::parse_uri(s)
        } else {
            Self::parse_non_uri(s)
        };
        debug!("RemoteName: {:?}", ret);
        ret
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
            host: HostInfo::Name("reterminal.local".to_string()),
            port: None,
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "mito@reterminal.local:/home/mito/";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: HostInfo::Name("reterminal.local".to_string()),
            port: None,
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "mito@[fe80::a00:27ff:fe0e:8c0c]:/home/mito/";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: HostInfo::Ip("fe80::a00:27ff:fe0e:8c0c".parse().unwrap()),
            port: None,
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "mito@[::1]:/home/mito/";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: HostInfo::Ip("::1".parse().unwrap()),
            port: None,
            path: Some(Path::new("/home/mito").into()),
        };
        assert_eq!(r, k);

        let s = "reterminal.local:";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: None,
            host: HostInfo::Name("reterminal.local".to_string()),
            port: None,
            path: None,
        };
        assert_eq!(r, k);

        let s = " mito @reterminal.local: ";
        let r: RemoteName = s.parse().unwrap();
        let k = RemoteName {
            user: Some("mito".to_string()),
            host: HostInfo::Name("reterminal.local".to_string()),
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

    #[test]
    // scp://[user@]host[:port][/path]　の解析テスト
    fn test_from_str_remotename_uri() {
        let s = "scp://name@hostname.hoge:22/test_path";
        let a = RemoteName {
            user: Some("name".to_string()),
            host: HostInfo::Name("hostname.hoge".to_string()),
            port: Some(22),
            path: Some(std::path::PathBuf::from("/test_path")),
        };
        let r: RemoteName = s.parse().unwrap();
        assert_eq!(r, a);

        let s = "scp://name@192.168.0.1:22/test_path/path";
        let a = RemoteName {
            user: Some("name".to_string()),
            host: HostInfo::Ip("192.168.0.1".parse().unwrap()),
            port: Some(22),
            path: Some(std::path::PathBuf::from("/test_path/path")),
        };
        let r: RemoteName = s.parse().unwrap();
        assert_eq!(r, a);

        let s = "scp://[::1]:22/test";
        let a = RemoteName {
            user: None,
            host: HostInfo::Ip("::1".parse().unwrap()),
            port: Some(22),
            path: Some(std::path::PathBuf::from("/test")),
        };
        let r: RemoteName = s.parse().unwrap();
        assert_eq!(r, a);

        let s = "scp://localhost";
        let a = RemoteName {
            user: None,
            host: HostInfo::Name("localhost".to_string()),
            port: None,
            path: None,
        };
        let r: RemoteName = s.parse().unwrap();
        assert_eq!(r, a);
    }
}
