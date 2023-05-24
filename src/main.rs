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
use std::fs::File;
//use log::debug;

fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::parse();

    let ssh = make_ssh_session(&opt).context("Failed to generate ssh session.")?;

    let path = make_remote_path(&opt, &ssh).context("Failed to generate remote path.")?;
    let options = make_mount_option(&opt);
    let mount_point = make_full_path(&opt.mount_point)?;

    // プロセスのデーモン化
    if opt.daemon {
        let stderr = File::create("err.out").unwrap();
        let daemonize = Daemonize::new().stderr(stderr);
        if let Err(e) = daemonize.start() {
            eprintln!("daemonization filed.(error: {})", e);
        }
    }
    // ファイルシステムへのマウント実行
    eprintln!("start fuse.");
    let fs = ssh_filesystem::Sshfs::new(ssh, &path);
    fuser::mount2(fs, mount_point, &options).context("Failed to mount FUSE.")?;
    Ok(())
}
