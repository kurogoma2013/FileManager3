# 保守ガイド

## 変更時の基本手順

1. `git status --short --branch` で作業ツリーを確認する。既存の変更を上書きしない。
2. 実装と関連する詳細仕様を同じ変更として更新する。
3. `cargo fmt --all`、`cargo test --offline`、`cargo clippy --all-targets --offline -- -D warnings`、`git diff --check` を実行する。テストはPostgreSQLに接続するため、事前に `docker compose up -d` でテスト用DBを起動する（[テストの実行](#テストの実行)を参照）。
4. バージョンを更新する。`Cargo.toml`を基準に、Web・Android・iOSの表示バージョンも合わせる。
5. 実行中のサービスを再起動し、待ち受けポートとHTTPSのヘルスチェックを確認する。
6. 変更内容を日本語のコミットメッセージでコミットする。

整合性検査はサービス稼働中でも実行できる。

```bash
cargo run -- check-integrity
```

検査で見つかったDB参照ファイルの欠落、サイズ・SHA-256不一致、孤立ファイル、PostgreSQL接続・スキーマ・外部キー整合性エラーは終了コード1で報告する。起動時に見つかった一時・孤立ファイルは削除せず、`FILE_STORAGE_DIR/.quarantine/` へ移動する。

## コードの責務

| 場所 | 責務 | 変更の目安 |
| --- | --- | --- |
| `src/main.rs` | ロギング、設定読込、DB接続、HTTPS待ち受け。実行ファイル名は `FileManager`（`Cargo.toml` の `[[bin]]`） | 起動方法・TLS・ディレクトリ準備 |
| `src/routes.rs` | WebAuthn状態の構築、Axum Router、共通ミドルウェア | 画面/APIのパス追加・変更 |
| `src/handlers/` | 認証、案件、販売店、メモ、ファイル、ユーザーのHTTP処理 | APIの入力・権限・レスポンス |
| `src/app_state.rs` | DBマイグレーション、初期管理者作成、起動時のストレージ整合性確認 | 起動時のDB準備 |
| `src/db.rs` | PostgreSQL接続プールとトランザクション共通処理 | DB接続設定 |
| `src/storage.rs` | ファイル名の安全性判定 | ストレージの入力検証 |
| `src/integrity.rs` | DB・ストレージ検査、起動時の未確定ファイル隔離、隔離ディレクトリ管理 | 整合性検査・復旧 |
| `src/core.rs` | ロールと操作権限の判定 | 権限ルール |
| `src/templates.rs` / `src/templates/` | HTMLテンプレートと共通UI補正 | 画面構造・共通表示 |
| `src/specifications.rs` | `docs/*.md` の管理者向け表示 | 仕様書の表示方式 |
| `migrations/` | PostgreSQLスキーマ（`20260917000000_init.sql` に統合済み）。以降の変更は新しいマイグレーションとして追加する | テーブル・インデックス変更 |
| `docker-compose.yml` | 開発用PostgreSQLコンテナ | `docs/10_docker_postgresql.md` |

## ルートを追加する場合

- パスとHTTPメソッドは `src/routes.rs` に登録する。
- 認証・権限・入力検証はハンドラー側で行い、既存の `ApiError` と共通認証関数を利用する。
- APIのパス、権限、ステータス、データの更新日時への影響を `docs/04_api.md` と `docs/02_permissions.md` に記載する。
- ブラウザ画面を追加した場合は `docs/03_screens.md` と関連テンプレートの対応を更新する。

## テストの実行

テストはSQLiteを使わず、PostgreSQL上でテストごとに専用データベース（`filemanager_test_<作成時刻>_<ランダム値>`）を作成して実行する。接続先は次の順で決定する。

1. 環境変数 `TEST_DATABASE_URL`（管理用接続URL。同じサーバー上にテストDBを作成する）
2. 環境変数 `POSTGRES_PASSWORD`、または `.env` の `POSTGRES_PASSWORD` を使い、`postgres://filemanager:<パスワード>@127.0.0.1:5432/filemanager` に接続する（ホストとポートは `TEST_DATABASE_HOST` で変更できる）

```bash
docker compose up -d
cargo test --offline
```

作成から1時間以上経過したテストDBは、次回のテスト実行時に自動で削除する。

## 動作確認

ローカルHTTPSの確認例:

```bash
cargo test --offline
cargo clippy --all-targets --offline -- -D warnings
curl --insecure --fail --silent --show-error https://127.0.0.1:3000/
lsof -nP -iTCP:3000 -sTCP:LISTEN
```

本番の更新は次の[本番サーバーの更新手順](#本番サーバーの更新手順スクリプト)に従い `scripts/update.sh` を使用する。Ubuntu、systemd、Nginx、Let's Encryptの構築手順は [Ubuntu・Let's Encrypt 設定](09_ubuntu_letsencrypt.md) にまとめている。リバースプロキシにApacheを使う場合は [Ubuntu・Apache 設定](11_ubuntu_apache.md) を参照する。

## 本番サーバーの更新手順（スクリプト）

Ubuntu + systemd 環境では、手動で `git pull`・ビルド・再起動を行わず、`scripts/update.sh` で一括更新する。スクリプトは次を順に実行し、途中で失敗した場合はサービスを再起動して終了コード1で停止する。

1. 前提確認: root実行、`Cargo.toml` の存在、実行ユーザーの存在、必要コマンド（`cargo` `curl` `git` `pg_dump` `runuser` `systemctl` `tar` など）、ローカル変更が無いこと、サービスが稼働中であること
2. `git pull --ff-only origin <ブランチ>` でソースコードを取得する（未追跡ファイルは保持したまま更新する）
3. コミットが進んだ場合のみ `cargo build --locked --release` でリリースビルドする
4. サービスを停止し、PostgreSQLの論理バックアップ（`pg_dump --format=custom`）とストレージの `tar.gz` を `FILEMANAGER_BACKUP_DIR/filemanager-<日時>/` に保存し、`metadata.txt`（日時・コミット・DB名・ストレージ）と `manifest.sha256` を書き出す
5. サービスを起動し、稼働状態と `FILEMANAGER_HEALTH_URL` へのHTTPS疎通を確認する
6. 更新前後のコミットと `systemctl status` の先頭を表示する

### 実行例

```bash
cd /opt/filemanager/source
sudo ./scripts/update.sh
```

ブランチやパスを変える場合は環境変数で指定する。

```bash
sudo FILEMANAGER_UPDATE_BRANCH=main \
     FILEMANAGER_DATA_DIR=/var/lib/filemanager/data \
     FILEMANAGER_BACKUP_DIR=/var/backups/filemanager \
     ./scripts/update.sh
```

| 環境変数 | 既定値 | 用途 |
| --- | --- | --- |
| `FILEMANAGER_SERVICE` | `filemanager` | systemdのサービス名 |
| `FILEMANAGER_RUN_USER` | `filemanager` | `git pull`・`cargo build`・`pg_dump` を実行するユーザー |
| `FILEMANAGER_UPDATE_BRANCH` | 現在のブランチ | 取得するブランチ |
| `FILEMANAGER_DATA_DIR` | `/var/lib/filemanager/data` | データディレクトリ |
| `FILEMANAGER_STORAGE_DIR` | `$FILEMANAGER_DATA_DIR/storage` | バックアップ対象のストレージ |
| `FILEMANAGER_DB_NAME` | `filemanager` | `pg_dump` 対象のデータベース名 |
| `FILEMANAGER_BACKUP_DIR` | `/var/backups/filemanager` | バックアップの保存先 |
| `FILEMANAGER_HEALTH_URL` | `https://127.0.0.1:3000/` | 更新後のヘルスチェックURL |

### 失敗時の確認

- `ローカル変更があります`: サーバー上で編集したファイルをコミットまたは `git stash` で退避してから再実行する
- `サービスが稼働していません`: `sudo systemctl start filemanager` で起動してから再実行する（停止中の更新は対象外）
- ビルド失敗: サービスは停止前なので稼働を継続している。`cargo build --locked --release` のログを確認する
- ヘルスチェック失敗: サービスは起動済みなので `journalctl -u filemanager -n 50` とポート・証明書を確認する。502の切り分けは [Ubuntu・Let's Encrypt 設定](09_ubuntu_letsencrypt.md) を参照する
- 巻き戻す場合: `git checkout <更新前コミット>` の後にリリースビルドと再起動を行い、必要なら `FILEMANAGER_BACKUP_DIR` の `database.dump` を `pg_restore`、`storage.tar.gz` を展開して復元する
