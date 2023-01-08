mod ssh_filesystem;

use clap::Parser;
use dns_lookup::lookup_host;
use log::debug;
use ssh2::Session;
use std::net::TcpStream;

fn main() {
    let opt = Opt::parse();
    env_logger::init();
    let options = vec![fuser::MountOption::FSName("sshfs".to_string())];

    println!("\"{}\"をマウントする予定", opt.remote);
    // 今は、固定接続先に、固定接続
    let address = lookup_host("reterminal.local").unwrap();
    if address.is_empty() {
        panic!("not found");
    }
    let socketaddr = std::net::SocketAddr::from((address[0], 22));
    debug!("接続先: {:?}", socketaddr);
    let tcp = TcpStream::connect(socketaddr).unwrap();
    let mut ssh = Session::new().unwrap();
    ssh.set_tcp_stream(tcp);
    ssh.handshake().unwrap();
    let key = std::path::Path::new("/home/mito/.ssh/id_rsa");
    ssh.userauth_pubkey_file("mito", None, key, None).unwrap();
    let fs = ssh_filesystem::Sshfs::new(ssh);

    fuser::mount2(fs, opt.mount_point, &options).unwrap();
}

/// コマンドラインオプション
#[derive(Parser)]
#[command(author, version, about)]
struct Opt {
    /// 接続先のアドレスまたはDNS名
    remote: String,
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

#[test]
fn verify_cli() {
    use clap::CommandFactory;
    Opt::command().debug_assert()
}
