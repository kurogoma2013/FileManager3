# API仕様



## 画面・認証

| メソッド | パス | 内容 |
| --- | --- | --- |
| GET | `/`、`/login` | ログイン / 案件検索画面 |
| GET | `/projects`、`/projects/{id}` | 案件検索結果 / 案件詳細画面 |
| GET | `/admin`、`/admin/*` | 管理メニュー・各管理画面 |
| GET | `/admin/specifications*` | 管理者専用の仕様書一覧・本文 |
| GET | `/logout` | ログアウト |
| POST | `/login` | パスワードログイン（Cookie設定） |
| GET | `/auth/google` | Google OAuthログイン開始（設定済みの場合） |
| GET | `/auth/google/callback` | Google OAuthコールバック、既存セッションCookieを設定 |
| GET | `/api/me` | ログインユーザー情報取得 |
| POST | `/api/logout-on-close` | ウィンドウ終了時ログアウト |
| GET | `/api/access-urls` | PC・LAN用URL取得 |
| GET | `/api/android/latest` | Android最新版のバージョン、APK URL、SHA-256、サイズ、更新内容を取得（未配置時は404） |
| GET | `/android/filemanager-android-release.apk` | 署名済みAndroid APKを取得（未配置時は404） |
| POST | `/api/passkeys/*` | パスキー登録・ログイン |

## データ操作

| 対象 | パス | 内容 |
| --- | --- | --- |
| 案件 | `/api/projects`、`/api/projects/{id}`、`/api/deleted/projects/{id}` | 検索、登録、更新、論理削除、復元、削除済み案件の物理削除。任意のメールアドレスを扱う |
| 案件メモ | `/api/projects/{id}/notes*` | 一覧、追加、削除、復元、削除済みメモの物理削除 |
| ファイル | `/api/projects/{id}/files`、`/api/files/{id}*`、`/api/deleted/files/{id}` | アップロード、取得、移動、タグ更新、削除、削除済みファイルの物理削除、版取得、一括ダウンロード（複数選択時は`selected-files.zip`）。`GET /api/files/{id}/audit` は管理者だけが操作ユーザーを確認できる |
| 販売店 | `/api/dealers`、`/api/dealers/{id}*`、`/api/deleted/dealers/{id}` | 一覧、登録、更新、論理削除、復元、削除済み販売店の物理削除。任意のメールアドレスを扱う |
| 担当者 | `/api/dealers/{name}/contacts`、`/api/contacts/{id}`、`/api/deleted/contacts/{id}` | 一覧、登録、更新、削除、削除済み担当者の物理削除。氏名・電話番号・任意のメールアドレスを扱う |
| 販売店メモ | `/api/dealers/{id}/notes*` | 一覧、追加、削除、復元、削除済みメモの物理削除 |
| ユーザー | `/api/users*`、`/api/deleted/users*` | 一覧、登録、更新、論理削除、削除済みユーザーの一覧・物理削除、パスキー管理。ユーザー削除は管理者のみ利用でき、管理者以外による管理ユーザーの削除は403で拒否する |

### 補足
- **日時**: APIが返す画面表示用の登録日時・更新日時・削除日時・メモ日時は日本時間（JST、UTC+9）。`session_expires_at` は `+09:00` 付きISO 8601形式で返す。データベース内の日時はUTCで保存する。
- **権限**: 物理削除APIは管理者のみ利用でき、`deleted_at` が設定されたデータまたは `active = FALSE` のユーザーだけを対象にする。案件・ファイルの物理削除では共有参照がなくなった物理ファイルを先に`.quarantine/`へ隔離し、その後DBレコードを削除する。
- **ファイル監査情報**: `GET /api/files/{id}/audit` は管理者のみ利用でき、対象ファイルと同名の各版について、アップロード者・論理削除者・各日時を返す。非管理者は403で拒否する。
- **認証**: セッションCookie、パスキー、またはGoogle OAuth。Basic認証は不使用。セッションの有効期限は1時間で、期限到達後は自動的にログアウトする。未認証時は401（`WWW-Authenticate` ヘッダーなし）。Google OAuthはstateを10分間・1回限りで検証し、メールアドレス確認済みのGoogleアカウントだけを連携する。
- **案件検索パラメータ**: `search`（または `q`）でスペース区切りAND検索。`sort`（`updated_desc`、`updated_asc`、`name_asc`、`name_desc`、`number_asc`、`number_desc`）に対応。
- **ファイルアップロード**: `multipart/form-data` の `file` フィールドを必須とし、`category=documents` または `category=pictures` で保存先タブを指定できる。`FileItem.created_at` はアップロード日時をJST（`YYYY-MM-DD HH:MM:SS`）で返し、画面の書類・写真一覧に表示する。書類では画像・動画も書類として保存でき、画像はWebPへ変換して書類一覧から写真と同じサムネイル・プレビュー表示を行い、動画は受信内容をそのまま保存する。写真タブの画像もWebPへ変換してからハッシュを計算する。パス区切りや絶対パスなどの危険なファイル名は保存前に400で拒否する。同一案件内で同名かつ同一内容の場合は既存ファイルを200で返し、同名別内容の場合は旧版を`file_histories`へ退避して新しい版番号を採番する。内容ハッシュが一致する別名ファイルは物理ストレージを共有する。`files.uploaded_by` にアップロード実行ユーザーID、`files.deleted_by` に論理削除実行ユーザーIDを内部保存し、既存レコードはNULLを許容する。
- **写真タグ更新**: `PUT /api/files/{id}/tag` にJSON `{ "tag": "タグ文字列" }` を送信する。前後の空白はサーバー側で除去され、空文字でタグを削除できる。admin/member、および写真に限りviewerが更新でき、更新後の`FileItem`を返す。
- **ユーザー一覧**: `UserSummary.updated_at` はユーザーの更新日時をJST（`YYYY-MM-DD HH:MM:SS`）で返す。ユーザーのロール、パスワード、論理削除状態、WebAuthn ID、パスキーが変更された時刻を更新日時として管理する。
