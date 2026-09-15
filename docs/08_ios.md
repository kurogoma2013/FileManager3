# iOS版 FileManager3

`ios/` に配置されたSwiftUI製iOSクライアントです。

## 機能・対応範囲
- ユーザー名・パスワードによるログイン（セッションCookie保持）
- 案件検索（単一キーワード・ソート対応）、案件詳細表示
- 案件詳細は「案件情報」「書類」「写真」のタブで切り替える。書類・写真タブにアップロードボタンを配置し、画像は2列のサムネイルで表示する。画像をタップするとQuick Lookでプレビューを表示し、動画は動画アイコンで表示する。写真カードにはタグを表示し、オンライン時はタグの追加・編集・削除に対応する。写真・動画・書類の一覧表示、端末からの写真・動画（MP4/M4V/MOV/WebM/OGV）アップロードに対応する。
- admin/memberでログインした場合は管理メニューを表示し、案件管理・販売店管理を開ける。adminはユーザー管理と仕様書も開ける。
- オフライン表示: 案件詳細で選択したファイル本体と案件情報をアプリ専用領域へ保存し、案件IDごとに保存期間（1〜3650日）を管理する。通信不能時は保存済み案件を読み取り専用で表示し、タグを含む写真情報を表示するが、タグ編集は行わない。保存済みファイルをQuick Lookで開く。ログイン画面の「保存済み案件をオフライン表示」から利用し、「オフラインキャッシュを削除」で全削除できる。

## ビルド
- 環境: Xcode 26、Swift 6、iOS 17以上
```bash
xcodebuild -project ios/FileManager3IOS.xcodeproj \
  -scheme FileManager3IOS \
  -sdk iphonesimulator \
  -configuration Debug \
  -derivedDataPath /tmp/filemanager3-ios-derived \
  CODE_SIGNING_ALLOWED=NO build
```

## 接続・制約
- HTTPS必須（端末側で信頼済みの証明書が必要）
- パスキーは現在未対応（パスワードログインを使用）
