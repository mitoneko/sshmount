pub mod remote_host;

use anyhow::{anyhow, Context};
use clap::Parser;
use std::path::PathBuf;

use remote_host::RemoteName;

/// コマンドラインオプション
#[derive(Parser)]
#[command(author, version, about)]
pub struct Opt {
    /// Distination [user@]host:[path] or scp://[user@]host[:port][/path]
    pub remote: RemoteName,
    /// Path to mount
    #[arg(value_parser = exist_dir)]
    pub mount_point: String,
    /// Path to config file
    #[arg(short = 'F', long)]
    pub config_file: Option<PathBuf>,
    /// Login name
    #[arg(short, long)]
    pub login_name: Option<String>,
    /// File name of secret key file
    #[arg(short, long)]
    pub identity: Option<PathBuf>,
    /// Port no
    #[arg(short, long)]
    pub port: Option<u16>,
    /// Read only
    #[arg(short, long)]
    pub readonly: bool,
    /// Not executable
    #[arg(long)]
    pub no_exec: bool,
    /// Do not change access date and time(atime)
    #[arg(long)]
    pub no_atime: bool,
    /// run in daemon mode
    #[arg(short, long)]
    pub daemon: bool,
}

/// 指定されたディレクトリが存在し、中にファイルがないことを確認する。
fn exist_dir(s: &str) -> anyhow::Result<String> {
    match std::fs::read_dir(s) {
        Ok(mut dir) => match dir.next() {
            None => Ok(s.to_string()),
            Some(_) => Err(anyhow!("Mount destination directory is not empty.")),
        },
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => Err(anyhow!("The mount directory does not exist.")),
            std::io::ErrorKind::NotConnected => Err(anyhow!(
                "The network of the mount directory is disconnected. (Did you forget to umount?)."
            )),
            _ => Err(e).context("Unexpected error.(check mount directory)"),
        },
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
}
