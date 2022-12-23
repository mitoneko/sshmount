use clap::Parser;

#[derive(Parser)]
#[command(author, version, about)]
struct Opt {
    /// 接続先のアドレスまたはDNS名
    remote: String,
    /// マウント先のパス
    mount_point: String,
}

fn main() {
    let opt = Opt::parse();

    println!("{}", opt.remote);
    println!("{}", opt.mount_point);
}
