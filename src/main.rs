mod cmdline_opt;
mod fuse_util;
mod ssh_connect;
mod ssh_filesystem;

use anyhow::{Context, Result};
use clap::Parser;
use cmdline_opt::Opt;
use daemonize::Daemonize;
use fuse_util::{make_full_path, make_mount_option, make_remote_path};
use log::{error, info};
use ssh_connect::make_ssh_session;
use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn main() -> Result<()> {
    //env_logger::init();
    let opt = Opt::parse();

    systemd_journal_logger::JournalLog::new()
        .expect("Fail initialize logger")
        .with_syslog_identifier("sshmount".to_string())
        .install()
        .expect("Fail install logger");
    match opt.debug {
        true => log::set_max_level(log::LevelFilter::Debug),
        false => log::set_max_level(log::LevelFilter::Info),
    }

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
            error!("daemonization filed.(error: {})", e);
            eprintln!("daemonization filed.(error: {})", e);
        }
    }
    // ファイルシステムへのマウント実行
    let fs = ssh_filesystem::Sshfs::new(ssh, &path).map_err(|e| {
        error!("Failed to create Sshfs.(error: {})", e);
        anyhow::anyhow!("Failed to create Sshfs.(error: {})", e)
    })?;
    //fuser::mount(fs, mount_point, &config).context("Failed to mount FUSE.")?;
    let handle = fuser::spawn_mount(fs, mount_point, &config).map_err(|e| {
        error!("Failed to mount FUSE.(error: {})", e);
        anyhow::anyhow!("Failed to mount FUSE.(error: {})", e)
    })?;
    info!("FUSE mounted successfully.");
    match handle.join() {
        Ok(()) => {
            info!("FUSE unmounted successfully.");
            Ok(())
        }
        Err(e) => {
            error!("Failed to unmount FUSE.(error: {:?})", e);
            eprintln!("Failed to unmount FUSE.(error: {:?})", e);
            Err(anyhow::anyhow!("Failed to unmount FUSE.(error: {:?})", e))
        }
    }
}
