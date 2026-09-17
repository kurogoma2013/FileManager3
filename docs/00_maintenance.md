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
| `src/main.rs` | ロギング、設定読込、DB接続、HTTPS待ち受け | 起動方法・TLS・ディレクトリ準備 |
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

テストはSQLiteを使わず、PostgreSQL上でテストごとに専用データベース（`filemanager3_test_<作成時刻>_<ランダム値>`）を作成して実行する。接続先は次の順で決定する。

1. 環境変数 `TEST_DATABASE_URL`（管理用接続URL。同じサーバー上にテストDBを作成する）
2. 環境変数 `POSTGRES_PASSWORD`、または `.env` の `POSTGRES_PASSWORD` を使い、`postgres://filemanager3:<パスワード>@127.0.0.1:5432/filemanager3` に接続する（ホストとポートは `TEST_DATABASE_HOST` で変更できる）

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

本番の更新は `scripts/update.sh` を使用する。Ubuntu、systemd、Nginx、Let's Encryptの構築手順は [Ubuntu・Let's Encrypt 設定](09_ubuntu_letsencrypt.md) にまとめている。リバースプロキシにApacheを使う場合は [Ubuntu・Apache 設定](11_ubuntu_apache.md) を参照する。
