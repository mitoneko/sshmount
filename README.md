[日本語はこちら](README-ja.md)

# Overview
　This application is used to mount the directory to which ssh connection is made on a linux system.

# Install [cargo].
 - Prepare a development environment for rust. [Install Rust](https://www.rust-lang.org/ja/tools/install)
 - Run "cargo install sshmount".

# install [manual].
 - Prepare a development environment for Rust. [Install Rust](https://www.rust-lang.org/ja/tools/install)
 - Clone the repository.
 - In the cloned directory, execute "cargo build --release".
 - Copy "target/release/sshmount" to an appropriate directory and execute it directly.

# Usage.

```
Mount the directory to which the ssh connection is made.

Usage: sshmount [OPTIONS] <REMOTE> <MOUNT_POINT>

Arguments:
  <REMOTE>       Distination [user@]host:[path]
  <MOUNT_POINT>  Path to mount

Options:
  -F, --config-file <CONFIG_FILE>  Path to config file
  -l, --login-name <LOGIN_NAME>    Login name
  -i, --identity <IDENTITY>        File name of secret key file
  -p, --port <PORT>                Port no [default: 22]
  -r, --readonly                   Read only
      --no-exec                    Not executable
      --no-atime                   Do not change access date and time(atime)
  -h, --help                       Print help
  -V, --version                    Print version

```

 - While running, it blocks the console, so please add "&" at the end and run it in backgroud or work on a separate console.
 - If you no longer need it after mounting, dismount it with "umount <MOUNT_POINT>".
   * If you force sshmount to terminate, for example by closing the console on which sshmount is running, a halfway mounted state will remain. In this case, please use "umount <MOUNT_POINT>" to dismount.
 - As for the config file, it conforms to ssh. The default is "$HOME/.ssh/config".
 - Please prepare an empty directory for the mount destination directory.
 - As for symbolic links in remote directories, those that go through directories higher than the mount point cannot refer to the link destination.
   * If an absolute path is specified, it always goes through the root, so it cannot be referenced except when the root is mounted.
   * If a path that is also valid on the local side is used as the link destination, the file on the local side is referenced.
 - This utility can be run with user privileges. (no sudo required)
   * When run with sudo, it will attempt to log in remotely as root when connecting as the default user.
 - The user and group names of the files in the mounted directory will be displayed as the user and group names on the local side. Note, however, that > and permission checks are performed with the user name on the remote side that you specified when connecting.

# License.
　Conforms to the Apache License 2.0.
