//! コマンドラインオブションにおけるRemoteHost構造体

use fluent_uri::{component::Host, Uri};
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
        let uri = Uri::parse(s)?;
        let info = uri
            .authority()
            .ok_or(ErrorRemoteName::NotFoundHostInformationInURI)?;
        // ポート番号の取得
        let port = info
            .port_to_u16()
            .map_err(|_| ErrorRemoteName::InvalidPortNo)?;
        // ホストアドレスの取得
        let host = match info.host_parsed() {
            Host::Ipv4(ip) => HostInfo::Ip(ip.into()),
            Host::Ipv6(ip) => HostInfo::Ip(ip.into()),
            _ => HostInfo::Name(info.host().to_string()),
        };
        // パス名の取得
        let path_str = uri.path().as_str();
        let path = if path_str.is_empty() {
            None
        } else {
            Some(
                path_str
                    .trim()
                    .parse::<std::path::PathBuf>()
                    .map_err(|_| ErrorRemoteName::InvalidPath)?,
            )
        };
        // ユーザー名の取得
        let user = info.userinfo().map(|s| s.as_str().trim().to_string());

        Ok(Self {
            user,
            host,
            port,
            path,
        })
    }

    /// remote引数の解析(URI形式でない場合)
    fn parse_non_uri(s: &str) -> Result<Self, ErrorRemoteName> {
        let mut rest_str = s.trim();
        // ユーザー名の取得
        let user = match rest_str.split_once("@") {
            Some((u, r)) => {
                rest_str = r;
                Some(u.trim().to_string())
            }
            None => None,
        };
        // ホストアドレスの取得
        let host_str: &str;
        // IPv6の判定
        rest_str = rest_str.trim_start();
        if rest_str.starts_with('[') {
            let (ip6_str, rest) = match rest_str.split_once(']') {
                Some((l, r)) => (l.trim_start_matches('['), r),
                None => return Err(ErrorRemoteName::MissingClosingBracketInIP6v),
            };
            host_str = ip6_str.trim();
            let (_, r) = rest.split_once(":").ok_or(ErrorRemoteName::NoColon)?;
            rest_str = r;
        } else {
            let (h, r) = rest_str.split_once(':').ok_or(ErrorRemoteName::NoColon)?;
            if h.trim().is_empty() {
                return Err(ErrorRemoteName::NoHostName);
            }
            host_str = h.trim();
            rest_str = r;
        }

        let try_ip = host_str.parse::<IpAddr>();
        let host = match try_ip {
            Ok(addr) => HostInfo::Ip(addr),
            Err(_) => HostInfo::Name(host_str.to_string()),
        };

        // パス名の取得
        rest_str = rest_str.trim();
        let path = if rest_str.is_empty() {
            None
        } else {
            Some(
                rest_str
                    .parse::<std::path::PathBuf>()
                    .map_err(|_| ErrorRemoteName::InvalidPath)?,
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
//#[error("The format of the host to connect to is \"[user@]host:[path]\" or \"scp://[user@]host[:port][/path]\".")]
pub enum ErrorRemoteName {
    #[error("URI parse error.")]
    UriParse(#[from] fluent_uri::ParseError),
    #[error("Host information not found at the specified URI.")]
    NotFoundHostInformationInURI,
    #[error("Invalid port number.")]
    InvalidPortNo,
    #[error("Invalid path name")]
    InvalidPath,
    #[error("The closing bracket is missing from IPv6.")]
    MissingClosingBracketInIP6v,
    #[error("In the host information, there is no subsequent colon (:).")]
    NoColon,
    #[error("No hostname specified.")]
    NoHostName,
}

#[cfg(test)]
mod test {
    use fluent_uri::ParseErrorKind;

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
        assert_eq!(r, Err(ErrorRemoteName::NoColon));

        let s = "mito@reterminal.local";
        let r: Result<RemoteName, ErrorRemoteName> = s.parse();
        assert_eq!(r, Err(ErrorRemoteName::NoColon));

        let s = " mito @: ";
        let r: Result<RemoteName, ErrorRemoteName> = s.parse();
        assert_eq!(r, Err(ErrorRemoteName::NoHostName));
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

    #[test]
    fn test_from_str_with_extra_space() {
        let s = "name@192.168.0.1:  /test_path/path   ";
        let a = RemoteName {
            user: Some("name".to_string()),
            host: HostInfo::Ip("192.168.0.1".parse().unwrap()),
            port: None,
            path: Some(std::path::PathBuf::from("/test_path/path")),
        };
        let r: RemoteName = s.parse().unwrap();
        assert_eq!(r, a);

        let s = "scp://[::1]:22/  /test";
        let r: Result<RemoteName, ErrorRemoteName> = s.parse();
        match r {
            Ok(_) => {
                unreachable!("この形式はエラーであるはず。");
            }
            Err(ErrorRemoteName::UriParse(e)) => {
                assert_eq!(e.kind(), ParseErrorKind::UnexpectedChar);
            }
            Err(e) => {
                unreachable!("何が起こった?({:?}", e);
            }
        }

        let s = "scp:// name @192.168.0.1:22/test_path/path";
        let r: Result<RemoteName, ErrorRemoteName> = s.parse();
        match r {
            Ok(_) => {
                unreachable!("この形式はエラーであるはず。");
            }
            Err(ErrorRemoteName::UriParse(e)) => {
                assert_eq!(e.kind(), ParseErrorKind::UnexpectedChar);
            }
            Err(e) => {
                unreachable!("何が起こった?({:?}", e);
            }
        }
    }
}
