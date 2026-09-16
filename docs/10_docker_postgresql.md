# Docker PostgreSQL

## 起動

`.env`を作成し、`POSTGRES_PASSWORD`にDBパスワードを設定します。

```bash
cp .env.example .env
# .env の POSTGRES_PASSWORD を変更
docker compose up -d
```

接続ポートは `docker-compose.yml` の `ports` で管理します。ホスト側にPostgreSQLを起動している場合は、使用中でないポートへ変更してから起動してください。

FileManager3から接続する場合は、次のURLを使用します。

```text
postgres://filemanager3:パスワード@127.0.0.1:＜docker-compose.ymlで公開したホスト側ポート＞/filemanager3
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
