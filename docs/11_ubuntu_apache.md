# Ubuntu 26.04 LTS での Apache を使用した HTTPS 設定

Ubuntu 26.04 LTS 上で、Nginx の代わりに Apache (apache2) をリバースプロキシとして FileManager3 を公開し、Let's Encrypt の証明書を使用する手順です。

FileManager3 はアプリ側でも HTTPS を必須としているため、次の構成にします。

```text
インターネット
    |
    | HTTPS :443
    v
Apache + Let's Encrypt
    |
    | HTTPS :3000（外部公開しない）
    v
FileManager3
```

VPS の準備、DNS、ファイアウォール、FileManager3 のビルド、内部用証明書、環境ファイル、systemd の設定は Nginx と共通です。[Ubuntu・Let's Encrypt 設定](09_ubuntu_letsencrypt.md) の **1〜9 を実施済み**であることを前提に、この文書では Apache 固有の手順だけを説明します。09 章の 2 で `nginx` をインストールした場合は、Apache と同じポートを使用するため停止しておきます。

```bash
sudo systemctl disable --now nginx
```

この手順では、取得済みの証明書に合わせて `goma2013.com` を使用します。別のドメインを使う場合は、手順中の `goma2013.com` をすべて実際のドメインへ置き換えてください。

| 値 | 例 | 説明 |
| --- | --- | --- |
| `goma2013.com` | `goma2013.com` | FileManager3 にアクセスする公開ドメイン |
| `127.0.0.1:3000` | `127.0.0.1:3000` | FileManager3 の内部 HTTPS 待ち受け（09 章の環境ファイルで設定） |

## 1. Apache をインストールしてモジュールを有効化する

```bash
sudo apt update
sudo apt install -y apache2

sudo a2enmod ssl proxy proxy_http headers rewrite
sudo systemctl enable --now apache2
sudo systemctl status apache2 --no-pager
```

| モジュール | 用途 |
| --- | --- |
| `ssl` | 公開側の HTTPS と、FileManager3 への HTTPS 接続（`SSLProxyEngine`） |
| `proxy` / `proxy_http` | `ProxyPass` によるリバースプロキシ |
| `headers` | `X-Forwarded-Proto` ヘッダーの付与 |
| `rewrite` | Certbot の `--redirect` が HTTP → HTTPS のリダイレクトに使用 |

内部 HTTPS が動作していることを確認します。

```bash
curl --insecure --fail https://127.0.0.1:3000/
```

## 2. Apache のサイト設定を作成する

```bash
sudo nano /etc/apache2/sites-available/filemanager.conf
```

証明書取得前は HTTP の VirtualHost を作成します。既定の `000-default` サイトに同じドメインが設定されている場合は、FileManager3 専用 VPS であることを確認してから無効化します。

```bash
sudo grep -RIn 'ServerName' /etc/apache2/sites-enabled
sudo a2dissite 000-default
```

他の Web サイトを `000-default` で運用している場合は、`a2dissite` を実行せず、そのサイト設定へ FileManager3 の設定を統合してください。

```apache
<VirtualHost *:80>
    ServerName goma2013.com

    # 100MB までのアップロードを許可（Nginx の client_max_body_size 100m 相当）
    LimitRequestBody 104857600

    # FileManager3 の内部用自己署名証明書へ loopback で接続するための設定
    SSLProxyEngine on
    SSLProxyVerify none
    SSLProxyCheckPeerCN off
    SSLProxyCheckPeerName off

    ProxyPreserveHost On
    ProxyTimeout 300
    ProxyPass / https://127.0.0.1:3000/
    ProxyPassReverse / https://127.0.0.1:3000/

    RequestHeader set X-Forwarded-Proto "https"

    ErrorLog ${APACHE_LOG_DIR}/filemanager-error.log
    CustomLog ${APACHE_LOG_DIR}/filemanager-access.log combined
</VirtualHost>
```

`SSLProxyVerify none` と `SSLProxyCheckPeer*` の無効化は、Apache から loopback の内部用自己署名証明書へ接続するための設定です（Nginx の `proxy_ssl_verify off` に相当）。外部サーバーを upstream にする場合は使用しないでください。

`X-Forwarded-For` は mod_proxy が自動で付与するため、明示的な設定は不要です。

有効化して、設定を確認します。

```bash
sudo a2ensite filemanager
sudo apachectl configtest
sudo systemctl reload apache2
curl --fail --silent --show-error -I http://goma2013.com
```

### 証明書を取得済みの場合

`sudo certbot certificates` で対象ドメインの証明書が表示される場合は、Certbot を再実行せず、その証明書を Apache に設定できます。`/etc/apache2/sites-available/filemanager.conf` を次の内容にします。

```apache
<VirtualHost *:80>
    ServerName goma2013.com
    Redirect permanent / https://goma2013.com/
</VirtualHost>

<VirtualHost *:443>
    ServerName goma2013.com

    SSLEngine on
    SSLCertificateFile /etc/letsencrypt/live/goma2013.com/fullchain.pem
    SSLCertificateKeyFile /etc/letsencrypt/live/goma2013.com/privkey.pem
    Include /etc/letsencrypt/options-ssl-apache.conf

    LimitRequestBody 104857600

    SSLProxyEngine on
    SSLProxyVerify none
    SSLProxyCheckPeerCN off
    SSLProxyCheckPeerName off

    ProxyPreserveHost On
    ProxyTimeout 300
    ProxyPass / https://127.0.0.1:3000/
    ProxyPassReverse / https://127.0.0.1:3000/

    RequestHeader set X-Forwarded-Proto "https"

    ErrorLog ${APACHE_LOG_DIR}/filemanager-error.log
    CustomLog ${APACHE_LOG_DIR}/filemanager-access.log combined
</VirtualHost>
```

`/etc/letsencrypt/options-ssl-apache.conf` は Certbot の Apache プラグインが作成する推奨 TLS 設定です。ファイルが存在しない場合は `Include` 行を削除してください。

設定後に確認・反映します。

```bash
sudo apachectl configtest
sudo systemctl reload apache2
curl --fail --silent --show-error -I https://goma2013.com
```

## 3. Let's Encrypt の証明書を取得する

証明書がまだない場合だけ、次を実行します。

Certbot を Snap からインストールします。

```bash
sudo snap install core
sudo snap refresh core
sudo snap install --classic certbot
sudo ln -s /snap/bin/certbot /usr/local/bin/certbot
```

Apache の設定を自動更新し、HTTP から HTTPS へリダイレクトします。Certbot は `filemanager.conf` を元に `filemanager-le-ssl.conf` を生成して有効化します。

```bash
sudo certbot --apache -d goma2013.com --redirect
```

ブラウザで次の URL を開きます。

```text
https://goma2013.com
```

## 4. 更新と動作を確認する

Let's Encrypt の自動更新が動作するか確認します。

```bash
sudo certbot renew --dry-run
sudo systemctl status apache2 --no-pager
curl --fail --silent --show-error -I https://goma2013.com
```

アップロードが失敗する場合は、`LimitRequestBody` が `104857600`（100MB）以上であることと、次のログを確認します。

```bash
sudo tail -n 100 /var/log/apache2/filemanager-error.log
sudo journalctl -u filemanager -n 100 --no-pager
```

`502 Proxy Error` や `SSL Proxy requested for ... but not enabled` が記録される場合は、`ssl` モジュールが有効で `SSLProxyEngine on` が VirtualHost 内に設定されていること、FileManager3 が `127.0.0.1:3000` で待ち受けていることを確認します。FileManager 側が起動していない場合の切り分けは [Ubuntu・Let's Encrypt 設定](09_ubuntu_letsencrypt.md) の「502 Bad Gateway が表示される場合」を参照してください。

```bash
sudo apachectl -M | grep -E 'ssl|proxy'
lsof -nP -iTCP:3000 -sTCP:LISTEN
```

## 5. 更新時の手順

FileManager3 本体の更新手順は Nginx 構成と同じです。[Ubuntu・Let's Encrypt 設定](09_ubuntu_letsencrypt.md) の 13 を参照してください。Apache の設定変更後は `sudo apachectl configtest` で構文を確認してから `sudo systemctl reload apache2` を実行します。

## 参考資料

- [Certbot の Ubuntu + Apache 手順](https://certbot.eff.org/instructions?os=ubuntufocal&ws=apache)
- [Apache mod_proxy](https://httpd.apache.org/docs/2.4/mod/mod_proxy.html)
- [Apache mod_ssl（SSLProxy* ディレクティブ）](https://httpd.apache.org/docs/2.4/mod/mod_ssl.html)
- [Let's Encrypt Getting Started](https://letsencrypt.org/getting-started/)
