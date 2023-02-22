mod cmdline_opt;
mod fuse_util;
mod ssh_connect;
mod ssh_filesystem;

use clap::Parser;
use cmdline_opt::Opt;
use fuse_util::{make_mount_option, make_remote_path};
use ssh_connect::make_ssh_session;
//use log::debug;

fn main() -> Result<(), String> {
    env_logger::init();
    let opt = Opt::parse();

    let ssh = make_ssh_session(&opt)?;

    let path = make_remote_path(&opt, &ssh)?;
    let options = make_mount_option(&opt);

    // ファイルシステムへのマウント実行
    let fs = ssh_filesystem::Sshfs::new(ssh, &path);
    fuser::mount2(fs, opt.mount_point, &options)
        .map_err(|e| format!("fuseのマウントに失敗しました。 -- {e}"))?;
    Ok(())
}
