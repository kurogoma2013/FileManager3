# Ubuntu 26.04 LTS での HTTPS 設定

KAGOYA CLOUD VPS の Ubuntu 26.04 LTS 上で FileManager3 を公開し、Let's Encrypt の証明書を使用する手順です。

FileManager3 はアプリ側でも HTTPS を必須としているため、次の構成にします。

```text
インターネット
    |
    | HTTPS :443
    v
Nginx + Let's Encrypt
    |
    | HTTPS :3000（外部公開しない）
    v
FileManager3
```

リバースプロキシに Nginx ではなく Apache を使う場合は、この文書の 1〜9 を実施した後に [Ubuntu・Apache 設定](11_ubuntu_apache.md) へ進んでください。

この手順では、取得済みの証明書に合わせて `goma2013.com` を使用します。別のドメインを使う場合は、手順中の `goma2013.com` をすべて実際のドメインへ置き換えてください。

| 値 | 例 | 説明 |
| --- | --- | --- |
| `goma2013.com` | `goma2013.com` | FileManager3 にアクセスする公開ドメイン |
| `VPSのIPv4アドレス` | `203.0.113.10` | KAGOYA VPS の固定 IPv4 アドレス |
| `filemanager` | `filemanager` | FileManager3 を実行する Linux ユーザー |
| `/opt/filemanager` | `/opt/filemanager` | FileManager3 の配置先 |

## 1. VPS に接続して OS を確認する

KAGOYA のコントロールパネルで VPS のグローバル IPv4 アドレスを確認し、SSH で接続します。

```bash
ssh root@VPSのIPv4アドレス
cat /etc/os-release
uname -m
```

Ubuntu 26.04 LTS であることを確認します。以降のコマンドは、`sudo` が使用できる管理者ユーザーで実行してください。

## 2. Ubuntu のパッケージと実行ユーザーを設定する

```bash
sudo apt update
sudo apt full-upgrade -y
sudo apt install -y \
  build-essential cargo git libpq-dev nginx openssl pkg-config postgresql ufw

sudo adduser --system --group --home /opt/filemanager \
  --shell /usr/sbin/nologin filemanager
sudo install -d -m 750 -o filemanager -g filemanager \
  /opt/filemanager /var/lib/filemanager/data/storage \
  /etc/filemanager/tls

sudo -u postgres psql <<'SQL'
CREATE USER filemanager WITH PASSWORD 'データベース用の強いパスワード';
CREATE DATABASE filemanager OWNER filemanager;
SQL
```

FileManager3はPostgreSQLを使用します。アプリケーション用DBユーザーとDBを作成し、パスワードは環境ファイルへ安全に設定します。

`filemanager` はサービス専用ユーザーです。既に専用ユーザーを作成済みの場合は、既存のユーザー名を以降のコマンドで使用してください。

## 3. DNS を設定する

ドメインの DNS に A レコードを追加し、VPS の固定 IPv4 アドレスを設定します。

```text
goma2013.com  A  VPSのIPv4アドレス
```

設定後、Ubuntu から名前解決を確認します。

```bash
getent ahostsv4 goma2013.com
```

IPv6 の AAAA レコードを設定していない場合、誤った AAAA レコードが残っていないことも確認してください。

## 4. KAGOYA のセキュリティグループを設定する

KAGOYA のコントロールパネルで、対象 VPS のセキュリティグループに次のルールを設定します。

| プロトコル | ポート | 接続元 | 用途 |
| --- | ---: | --- | --- |
| TCP | `22` | 管理者の固定 IP のみ | SSH |
| TCP | `80` | `0.0.0.0/0` | Let's Encrypt の HTTP 認証、HTTP リダイレクト |
| TCP | `443` | `0.0.0.0/0` | 公開 HTTPS |

IPv6 を使用する場合は `80` と `443` に `::/0` のルールも追加します。

`3000` 番ポートは公開しません。FileManager3 は `127.0.0.1:3000` で待ち受けさせます。

## 5. Ubuntu のファイアウォールを設定する

SSH 接続を維持したまま、次を実行します。SSH ポートを変更している場合は `22` を実際のポートに置き換えてください。

```bash
sudo ufw allow 22/tcp
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw enable
sudo ufw status verbose
```

KAGOYA のセキュリティグループと Ubuntu の UFW の両方で通信が許可されている必要があります。

## 6. FileManager3 を配置してビルドする

GitHub からソースコードを取得し、リリースビルドを作成します。

```bash
sudo -u filemanager git clone \
  https://github.com/kurogoma2013/FileManager3.git \
  /opt/filemanager/source

sudo -u filemanager sh -c \
  'cd /opt/filemanager/source && cargo build --locked --release'
```

ビルドが完了すると、実行ファイルは次の場所に作成されます。

```text
/opt/filemanager/source/target/release/FileManager
```

## 7. FileManager3 の内部用証明書を作成する

FileManager3 は HTTPS 起動が必要です。Nginx と FileManager3 は同じ VPS の loopback 接続なので、FileManager3 側には内部通信用の自己署名証明書を使用します。

以下の `filemanager` は実際の実行ユーザーに置き換えてください。サービスユーザーで手動起動する場合は、証明書をそのユーザーが読み取れる場所に作成してください。

```bash
sudo -u filemanager openssl req -x509 -nodes -newkey rsa:2048 \
  -days 3650 \
  -keyout /etc/filemanager/tls/backend.key \
  -out /etc/filemanager/tls/backend.crt \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"

sudo chmod 640 /etc/filemanager/tls/backend.key
```

`proxy_ssl_verify off` は、Nginx から loopback の内部用自己署名証明書へ接続するための設定です。外部サーバーを upstream にする場合は使用しないでください。

## 8. FileManager3 の環境ファイルを作成する

`/etc/filemanager/filemanager.env` を作成します。

```bash
sudo nano /etc/filemanager/filemanager.env
```

```text
DATABASE_URL=postgres://filemanager:データベース用の強いパスワード@127.0.0.1/filemanager
FILE_STORAGE_DIR=./data/storage
HOST=127.0.0.1
PORT=3000
ADMIN_PASSWORD=初回ログイン用の強いパスワード
SECURE_COOKIE=true
TLS_CERT_PATH=/etc/filemanager/tls/backend.crt
TLS_KEY_PATH=/etc/filemanager/tls/backend.key
WEBAUTHN_RP_ID=goma2013.com
WEBAUTHN_ORIGIN=https://goma2013.com
```

`WEBAUTHN_RP_ID` と `WEBAUTHN_ORIGIN` には、利用者がブラウザで開く公開ドメインを設定します。`localhost` や `:3000` は設定しません。

初回起動時は、空のPostgreSQLデータベースに `admin` ユーザーが作成されます。既存DBの管理者パスワードは起動時に上書きされません。

環境ファイルの権限を制限します。

```bash
sudo chown root:filemanager /etc/filemanager/filemanager.env
sudo chmod 640 /etc/filemanager/filemanager.env

sudo install -d -m 750 \
  -o filemanager -g filemanager \
  /var/lib/filemanager \
  /var/lib/filemanager/data \
  /var/lib/filemanager/data/storage
sudo chown -R filemanager:filemanager /var/lib/filemanager
```

## 9. systemd で FileManager3 を起動する

サービス定義を作成します。

```bash
sudo nano /etc/systemd/system/filemanager.service
```

```ini
[Unit]
Description=FileManager
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=filemanager
Group=filemanager
WorkingDirectory=/var/lib/filemanager
EnvironmentFile=/etc/filemanager/filemanager.env
ExecStart=/opt/filemanager/source/target/release/FileManager
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ReadWritePaths=/var/lib/filemanager

[Install]
WantedBy=multi-user.target
```

起動してログを確認します。

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now filemanager
sudo systemctl status filemanager --no-pager
sudo journalctl -u filemanager -n 100 --no-pager
```

内部 HTTPS を確認します。

```bash
curl --insecure --fail https://127.0.0.1:3000/
```

初回起動が成功し、管理者パスワードを設定できたことを確認したら、環境ファイルから初期パスワードを削除します。既存 DB では不要です。

```bash
sudo sed -i '/^ADMIN_PASSWORD=/d' /etc/filemanager/filemanager.env
sudo systemctl restart filemanager
```

## 10. Nginx を設定する

```bash
sudo nano /etc/nginx/sites-available/filemanager
```

証明書取得前は HTTP の server block を作成します。既定の `default` サイトに同じドメインが設定されている場合は、FileManager3 専用 VPS であることを確認してから無効化します。

```bash
sudo grep -RIn 'server_name' /etc/nginx/sites-enabled
if [ -L /etc/nginx/sites-enabled/default ]; then
  sudo unlink /etc/nginx/sites-enabled/default
fi
```

他の Web サイトを `default` で運用している場合は、`unlink` を実行せず、そのサイト設定へ FileManager3 の設定を統合してください。

```nginx
server {
    listen 80;
    listen [::]:80;

    server_name goma2013.com;

    client_max_body_size 100m;

    location / {
        proxy_pass https://127.0.0.1:3000;
        proxy_ssl_verify off;

        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;

        proxy_read_timeout 300s;
        proxy_send_timeout 300s;
    }
}
```

有効化して、設定を確認します。

```bash
sudo ln -s /etc/nginx/sites-available/filemanager \
  /etc/nginx/sites-enabled/filemanager

sudo nginx -t
sudo systemctl reload nginx
```

### 証明書を取得済みの場合

`sudo certbot certificates` で対象ドメインの証明書が表示される場合は、Certbot を再実行せず、その証明書を Nginx に設定できます。`/etc/nginx/sites-available/filemanager` を次の内容にします。

```nginx
server {
    listen 80;
    listen [::]:80;

    server_name goma2013.com;

    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    listen [::]:443 ssl;

    server_name goma2013.com;

    ssl_certificate /etc/letsencrypt/live/goma2013.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/goma2013.com/privkey.pem;

    client_max_body_size 100m;

    location / {
        proxy_pass https://127.0.0.1:3000;
        proxy_ssl_verify off;

        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;

        proxy_read_timeout 300s;
        proxy_send_timeout 300s;
    }
}
```

設定後に確認・反映します。

```bash
sudo nginx -t
sudo systemctl reload nginx
curl --fail --silent --show-error -I https://goma2013.com
```

## 11. Let's Encrypt の証明書を取得する

証明書がまだない場合だけ、次を実行します。

Certbot を Snap からインストールします。

```bash
sudo snap install core
sudo snap refresh core
sudo snap install --classic certbot
sudo ln -s /snap/bin/certbot /usr/local/bin/certbot
```

Nginx の設定を自動更新し、HTTP から HTTPS へリダイレクトします。

```bash
sudo certbot --nginx -d goma2013.com --redirect
```

ブラウザで次の URL を開きます。

```text
https://goma2013.com
```

## 12. 更新と動作を確認する

Let's Encrypt の自動更新が動作するか確認します。

```bash
sudo certbot renew --dry-run
sudo systemctl status nginx --no-pager
curl --fail --silent --show-error -I https://goma2013.com
```

アップロードが失敗する場合は、Nginx の `client_max_body_size` が `100m` 以上であることと、次のログを確認します。

```bash
sudo journalctl -u nginx -n 100 --no-pager
sudo journalctl -u filemanager -n 100 --no-pager
```

## 13. 更新時の手順

新しいバージョンを配置する場合は、まずPostgreSQLのダンプとストレージを同一更新単位で保存し、ソースコードを更新してビルドした後、サービスを再起動します。バックアップ先はアクセス制限した保存先を使用してください。

```bash
sudo install -d -m 750 /var/backups/filemanager
sudo -u filemanager pg_dump --format=custom --file=/var/backups/filemanager/database.dump filemanager
sudo tar -C /var/lib/filemanager/data -czf /var/backups/filemanager/storage.tar.gz storage
sudo systemctl stop filemanager
sudo -u filemanager git -C /opt/filemanager/source pull --ff-only
sudo -u filemanager sh -c \
  'cd /opt/filemanager/source && cargo build --locked --release'
sudo systemctl start filemanager
sudo systemctl status filemanager --no-pager
```

## 参考資料

- [KAGOYA CLOUD VPS セキュリティ](https://www.kagoya.jp/vps/feature/security/)
- [KAGOYA CLOUD VPS](https://www.kagoya.jp/vps/)
- [Certbot の Ubuntu + Nginx 手順](https://certbot.eff.org/instructions?os=ubuntufocal&ws=nginx)
- [Let's Encrypt Getting Started](https://letsencrypt.org/getting-started/)
