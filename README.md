# 概要
　linuxシステムにおいて、ssh接続先のディレクトリをマウントするためのアプリです。

# インストール
 - rustの開発環境を用意してください。[Rustをインストール](https://www.rust-lang.org/ja/tools/install)
 - リポジトリをクローンしてください。
 - クローンしたディレクトリで、「cargo build --release」を実行してください。
 - "target/release/sshmount"を適切なディレクトリにコピーして、直接実行してください。
 - 実行中は、コンソールをブロックするので、末尾に"&"をつけてバックグラウドで実行するか、別コンソールで作業をしてください。

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

 - マウント後、不要になった場合は、「umount <MOUNT_POINT>」でマウントを解除してください。
   * sshmountを実行中のコンソールを閉じるなど、sshmountを強制的に終了させると中途半端なマウント状態が残ります。この場合も、「umount <MOUNT_POINT>」で解除してください。
 - configファイルに関しては、sshに準拠します。デフォルトは、「$HOME/.ssh/config」を使用します。
 - マウント先のディレクトリは空のディレクトリを用意してください。
 - リモートディレクトリ内のシンボリックリンクに関しては、マウントポイントより上位のディレクトリを経由しているものは、リンク先の参照が出来ません。
   * 絶対パス指定の場合、必ずルートを経由するため、ルートをマウントした時以外は参照不可です。
   * ローカル側でも有効なパスがリンク先になっている場合、ローカル側のファイルを参照します。
 - このユーティリティは、ユーザー権限で実行可能です。(sudo不要)
   * sudo付きで実行すると、デフォルトユーザーで接続した時、rootでリモートにログインを試みます。
 - マウントしたディレクトリ内のファイルのユーザーとグループは、ローカル側でのユーザー名・グループ名が表示されます。ただし、権限のチェックは、接続時に指定したリモート側のユーザー名で実行されるので注意してください。

# ライセンス。
　Apache License 2.0に準拠します。
