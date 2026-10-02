mod cmdline_opt;
mod fuse_util;
mod ssh_connect;
mod ssh_filesystem;

use anyhow::{Context, Result};
use clap::Parser;
use cmdline_opt::Opt;
use daemonize::Daemonize;
use fuse_util::{make_full_path, make_mount_option, make_remote_path};
use ssh_connect::make_ssh_session;
//use log::debug;

fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::parse();

    let ssh = make_ssh_session(&opt).context("Failed to generate ssh session.")?;

    let path = make_remote_path(&opt, &ssh).context("Failed to generate remote path.")?;
    let options = make_mount_option(&opt);
    let mut config = fuser::Config::default();
    config.mount_options = options;

    let mount_point = make_full_path(&opt.mount_point)?;

    // プロセスのデーモン化
    if opt.daemon {
        let daemonize = Daemonize::new();
        if let Err(e) = daemonize.start() {
            eprintln!("daemonization filed.(error: {})", e);
        }
    }
    // ファイルシステムへのマウント実行
    let fs = ssh_filesystem::Sshfs::new(ssh, &path)?;
    fuser::mount(fs, mount_point, &config).context("Failed to mount FUSE.")?;
    Ok(())
}
