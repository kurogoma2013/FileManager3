# 設定・起動

設定は `config/settings.json` および環境変数で管理します（環境変数が優先）。

## 設定項目

| 環境変数 / キー | 説明 | 既定値 |
| --- | --- | --- |
| `DATABASE_URL` / `database_url` | PostgreSQL接続URL。パスワードに `#` `@` `/` `:` `?` `%` などの記号を含む場合は URL エンコードする（未エンコードだと起動時に `invalid port number` で失敗） | `postgres://filemanager:パスワード@127.0.0.1/filemanager` |
| `FILE_STORAGE_DIR` / `storage_dir` | 共通ファイル保存ディレクトリ（隔離先は配下の`.quarantine`） | `./data/storage` |
| `HOST` / `bind_address` | 待ち受けアドレス | `0.0.0.0` |
| `PORT` / `port` | 待ち受けポート | `3000` |
| `ADMIN_PASSWORD` | 初回起動時の管理者パスワード（必須） | - |
| `SECURE_COOKIE` | HTTPS専用Cookie設定（`true` 必須） | `true` |
| - | セッション有効期限 | 1時間 |
| - | 日時表示 | 日本時間（JST、UTC+9）。データベース内はUTCで保存 |
| `TLS_CERT_PATH` / `TLS_KEY_PATH` | TLS証明書・秘密鍵のPEMパス（必須） | - |
| `WEBAUTHN_RP_ID` | パスキーのRP ID（ホスト名） | 自動判定 / `localhost` |
| `WEBAUTHN_ORIGIN` | パスキーのOrigin（`https://...`） | 自動生成 |
| `GOOGLE_CLIENT_ID` / `google_client_id` | Google OAuthクライアントID | 未設定（Googleログイン非表示） |
| `GOOGLE_CLIENT_SECRET` / `google_client_secret` | Google OAuthクライアントシークレット | 未設定（Googleログイン非表示） |
| `GOOGLE_REDIRECT_URI` / `google_redirect_uri` | Google OAuthのコールバックURL | 未設定（Googleログイン非表示） |
| `FILEMANAGER_ANDROID_RELEASE_DIR` | Android APKと`latest.json`を配置するディレクトリ | `dist/android-release` |

## 起動コマンド

```bash
export SECURE_COOKIE=true
export TLS_CERT_PATH=/path/to/cert.pem
export TLS_KEY_PATH=/path/to/key.pem
export WEBAUTHN_RP_ID=filemanager.local
export WEBAUTHN_ORIGIN=https://filemanager.local:3000
export GOOGLE_CLIENT_ID=xxxxxxxx.apps.googleusercontent.com
export GOOGLE_CLIENT_SECRET=xxxxxxxx
export GOOGLE_REDIRECT_URI=https://filemanager.local:3000/auth/google/callback
cargo run
```

## 注意事項
- **HTTPS必須**: HTTPでの起動は拒否されます。
- **LANパスキー**: IPアドレス直接アクセスではパスキー不可のため、証明書に対応したホスト名でアクセスします。
- **データ初期化**: PostgreSQLの接続先・データベース・権限を事前に用意します。初回起動時は `ADMIN_PASSWORD` を指定します。既存DBの管理者パスワードは上書きされません。
- **Googleログイン**: Google CloudでOAuthクライアント（ウェブアプリケーション）を作成し、上記のリダイレクトURIを承認済みのリダイレクトURIへ登録します。3項目がすべて設定された場合だけログイン画面にボタンが表示されます。Googleで初回ログインしたユーザーは `member` 権限で作成され、既存ユーザーとメールアドレスが一致する場合はそのユーザーへ紐付けます。Googleから返されたメールアドレスが未確認の場合はログインできません。

## 更新スクリプト

Ubuntuのsystemd環境では、リポジトリの `scripts/update.sh` を使用して更新します。ソースコードの取得、リリースビルド、PostgreSQLの論理バックアップとストレージのバックアップ、サービス再起動、HTTPS疎通確認を自動で行います。

```bash
cd /opt/filemanager/source
sudo ./scripts/update.sh
```

整合性検査を手動実行する場合:

```bash
cargo run -- check-integrity
```
