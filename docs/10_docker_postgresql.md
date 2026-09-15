# Docker PostgreSQL

## 起動

`.env`を作成し、`POSTGRES_PASSWORD`にDBパスワードを設定します。

```bash
cp .env.example .env
# .env の POSTGRES_PASSWORD を変更
docker compose up -d
```

Dockerコンテナはホストの5433番ポートへ公開します。ネイティブPostgreSQLの5432番ポートと同時に利用できます。

FileManager3から接続する場合は、次のURLを使用します。

```text
postgres://filemanager3:パスワード@127.0.0.1:5433/filemanager3
```

## 状態確認

```bash
docker compose ps
docker compose exec filemanager3-postgres pg_isready -U filemanager3 -d filemanager3
```

## 停止

```bash
docker compose stop
```

データを保持したままコンテナを停止します。DBデータを削除する場合は、対象ボリュームを確認してから`docker compose down -v`を実行してください。
