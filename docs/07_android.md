# Android版 FileManager3

`android/` に配置されたネイティブJava Androidクライアントです。

## 機能・対応範囲
- ユーザー名・パスワードによるログイン（セッションCookie保持、成功時の204応答とログイン直後のセッション確認に対応、空欄入力を送信しない、401時は利用者向けメッセージを表示）
- 案件検索（単一キーワード・ソート対応）、案件詳細表示
- モバイルWeb版に合わせ、画面上部のタイトル・ログアウト、白いカード、青系のアクセント、ラベルと内容を縦方向に整理したレイアウトで表示する。検索はWeb版と同じ単一検索欄、検索結果件数、カード型の縦並びで表示する。
- 案件詳細は「案件情報」「書類」「写真」のタブで切り替える。書類・写真タブにアップロードボタンを配置し、画像は2列のサムネイルで表示する。画像をタップするとプレビューを表示し、動画は動画アイコンで表示する。写真カードにはタグを表示し、オンライン時はタグの追加・編集・削除に対応する。写真・動画・書類の一覧表示、端末からの写真・動画（MP4/M4V/MOV/WebM/OGV）アップロードに対応する。
- admin/memberでログインした場合は管理メニューを表示し、案件管理・販売店管理を開ける。adminはユーザー管理と仕様書も開ける。
- オフライン表示: 案件詳細で選択したファイル本体と案件情報をアプリ専用領域へ保存する。「この案件の選択ファイルを保存」を選択した時に保存期間（1〜3650日）を設定し、案件IDごとに管理する。通信不能時は保存済み案件を読み取り専用で表示し、タグを含む写真情報を表示するが、タグ編集は行わない。保存済みファイルを端末ビューアで開く。ログイン画面の「保存済み案件をオフライン表示」から利用し、「オフラインキャッシュを削除」で全削除できる。

## ビルド
- 環境: JDK 17、Android SDK Platform 36、Build Tools 36.1.0、Gradle Wrapper 9.7.1
```bash
cd android
./gradlew --no-daemon copyDebugApk
```
生成先: `dist/filemanager3-android-debug.apk`

### release署名ビルド
- `android/keystore.properties`（Git管理外）に`storeFile`、`storePassword`、`keyAlias`、`keyPassword`を設定する。
- keystore（`.jks`または`.keystore`）とパスワードは安全にバックアップし、リポジトリへ登録しない。
```bash
cd android
./gradlew --no-daemon copyReleaseApk
```
生成先: `dist/android-release/filemanager3-android-release.apk`
- アプリ更新時は同じ署名鍵を継続して使用する。署名鍵を紛失すると既存アプリを更新できない。

### ビルド環境
- JDK 17
- Android SDK Platform 36、Build Tools 36.1.0
- Gradle Wrapper 9.7.1（`gradle/wrapper/`に同梱）
- 初回のみ、Android SDKのライセンスを承認する。`android/local.properties`は各開発環境のSDKパスを指定するため、Git管理対象外とする。

## 接続・制約
- 既定の接続先: `https://goma2013.com`（アプリ内部で固定し、ログイン画面には表示しない）
- ログイン画面: Web版と同じ背景色・中央配置・カード型パネル・ブランド表示・入力欄・ボタン余白に揃える。オフライン表示などAndroid固有の操作はWeb版にないため、ログインカード内の追加ボタンとして提供する。
- 自動アップデート: 起動時に`GET /api/android/latest`で更新を確認し、更新があればHTTPSでAPKを取得する。取得後はSHA-256を検証してAndroidのインストール画面を開く。APKは`FILEMANAGER_ANDROID_RELEASE_DIR`（未設定時は`dist/android-release`）の`filemanager3-android-release.apk`を配布する。
- リリースビルド時に`latest.json`も同じディレクトリへ生成する。`latest.json`とAPKは同じリリースの組み合わせでサーバーへ配置する。
- APK配布: `copyReleaseApk`で作成した署名済みAPKをサーバーの配布ディレクトリへ配置する。既存アプリを更新するには、毎回同じ署名鍵を使用し、`versionCode`を増加させる。Android 8以降では初回のみ、このアプリからのインストール許可が必要で、インストールの最終確認は端末で行う。
- HTTPS必須（証明書が信頼されている必要があります）
- パスキーは現在未対応（パスワードログインを使用）
