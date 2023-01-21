# 概要
　linuxシステムにおいて、ssh接続先のディレクトリをマウントするためのアプリです。

# 制限
　現在のバージョンでは、マウント先のディレクトリは、リードオンリーです。  
　書き込みは出来ません。

# ライセンス。
　GPLv3に準拠します。

# インストール
　現在、インストーラー等は、付属していません。リポジトリをクローン後、「cargo build --release」でコンパイルの上、「./target/release/sshmount」を直接実行してください。「cargo run」で実行する場合は、コマンドへのオプションの前に「--」を付加してください。(これがないと、一部オプションが、cargoへのオプションとして認識され正常に実行できません。)

# 使用方法

```
ssh接続先のディレクトリをマウントする

Usage: sshmount [OPTIONS] <REMOTE> <MOUNT_POINT>

Arguments:
  <REMOTE>       接続先 [user@]host:[path]
  <MOUNT_POINT>  マウント先のパス

Options:
  -F, --config-file <CONFIG_FILE>  configファイルのパス指定
  -l, --login-name <LOGIN_NAME>    ログイン名
  -i, --identity <IDENTITY>        秘密キーファイル名
  -p, --port <PORT>                ポート番号 [default: 22]
  -h, --help                       Print help information
  -V, --version                    Print version information
```

 configファイルに関しては、sshに準拠します。デフォルトは、「$HOME/.ssh/config」を使用します。また、マウント先のディレクトリは空のディレクトリを用意してください。
