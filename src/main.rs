use clap::Parser;

fn main() {
    let opt = Opt::parse();

    println!("\"{}\"をマウントする予定", opt.remote);
    println!("\"{}\"にマウントされる予定", opt.mount_point);
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
