mod cmdline_opt;
mod fuse_util;
mod ssh_connect;
mod ssh_filesystem;

use anyhow::{Context, Result};
use clap::Parser;
use cmdline_opt::Opt;
use fuse_util::{make_mount_option, make_remote_path};
use ssh_connect::make_ssh_session;
//use log::debug;

fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::parse();

    let ssh = make_ssh_session(&opt).context("Failed to generate ssh session.")?;

    let path = make_remote_path(&opt, &ssh).context("Failed to generate remote path.")?;
    let options = make_mount_option(&opt);

    // ファイルシステムへのマウント実行
    let fs = ssh_filesystem::Sshfs::new(ssh, &path);
    fuser::mount2(fs, opt.mount_point, &options).context("Failed to mount FUSE.")?;
    Ok(())
}
