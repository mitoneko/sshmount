# 更新履歴
## v0.4.0 (2026-09-27)

- (internal)依存クレート群の更新。
- (internal)Rust Edition 2024への対応。
- (refactor)cargo clippyの警告に対応(主に、参照指定の扱いの変更)
- (optimize)Cargo.tomlの[profile-release]の指定変更(最適化関係の一連の変更)
- (feature)fuserクレートを0.18.0に変更。これに伴い、関数呼び出しのAPI変更への対応一式。
- (feature)接続時のマウントオプションにDefaultPermissionsを追加。
- (feature)ログの記録をjournalctlに対応。(env_loggerを廃止)
- (feature)起動オプションに--debugを追加。デバッグログの出力を可能にする。
- (bug) 多数のファイル削除が出た際、ファイルハンドルの扱いが不適切で削除が失敗するバグの修正(ファイルハンドルのキャッシュ追加)
- (bug) rename時のRENAME_NOREPLACEの解釈誤りを修正。rename先が存在する場合の処置を修正
- (bug) rmdir_errnoの修正。ディレクトリの削除時に、他のパターンがあったことで変換漏れが出ていた。
- (feature) 統合テスト環境の創設と、実実行によるテストの追加。cargo make testにて実行可能。
- (bug) ディレクトリのrename時に、配下のファイルアクセスが新ディレクトリ名でできない問題を修正。Inode実装の更新。

## v0.3.2 (2026-02-01)

- 機能追加: ssh_configパラメーターにて、複数のIdentityFileオプションを指定可能にした。

## v0.3.1 (2026-01-31)

- バグ修正: コマンドラインのremotehostパラメーターにて、IPv6アドレス指定時に正しく動作しないバグの修正。

## v0.3.0 (2026-01-11)

- 機能追加: コマンドラインのremotehostパラメーターにて、IPv6アドレスの指定を可能にした。
- 機能追加: コマンドラインのremotehostパラメーターにて、scp://[user@]host[:port][/path]形式での指定を可能にした。

## V0.2.1

- バグ修正: リモートへのアクセス時、ファイルへのアクセスが不可能になったバグの修正

## V0.2.0 

- 各種効率化のための改善
