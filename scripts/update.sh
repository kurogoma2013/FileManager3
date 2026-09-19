#!/usr/bin/env bash
set -euo pipefail

# FileManager3 の本番更新スクリプト（Ubuntu + systemd 用）
# 例: sudo FILEMANAGER_DATA_DIR=/var/lib/filemanager/data ./scripts/update.sh

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVICE_NAME="${FILEMANAGER_SERVICE:-filemanager}"
RUN_USER="${FILEMANAGER_RUN_USER:-filemanager}"
HEALTH_URL="${FILEMANAGER_HEALTH_URL:-https://127.0.0.1:3000/}"

log() {
  printf '[FileManager3] %s\n' "$*"
}

fail() {
  printf '[FileManager3] エラー: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "必要なコマンドが見つかりません: $1"
}

run_as_app_user() {
  if [ "$(id -u)" -eq 0 ]; then
    runuser -u "$RUN_USER" -- "$@"
  else
    "$@"
  fi
}

backup_data() {
  # バックアップ本体は scripts/backup.sh と共通。更新時は保持日数による世代削除を行わない
  FILEMANAGER_BACKUP_KEEP_DAYS=0 "$ROOT_DIR/scripts/backup.sh" || fail "バックアップに失敗しました"
}

require_command cargo
require_command curl
require_command git
require_command install
require_command runuser
require_command systemctl

[ "$(id -u)" -eq 0 ] || fail "このスクリプトは sudo または root で実行してください"
[ -f "$ROOT_DIR/Cargo.toml" ] || fail "Cargo.toml が見つかりません: $ROOT_DIR"
id "$RUN_USER" >/dev/null 2>&1 || fail "実行ユーザーが見つかりません: $RUN_USER"

cd "$ROOT_DIR"
branch="${FILEMANAGER_UPDATE_BRANCH:-$(git symbolic-ref --quiet --short HEAD || true)}"
[ -n "$branch" ] || fail "ブランチを判定できません。FILEMANAGER_UPDATE_BRANCH を指定してください"

if ! git diff --quiet || ! git diff --cached --quiet; then
  fail "ローカル変更があります。コミットまたは退避してから実行してください"
fi
if [ -n "$(git ls-files --others --exclude-standard)" ]; then
  log "警告: 未追跡ファイルは保持したまま更新します"
fi

systemctl is-active --quiet "$SERVICE_NAME" || fail "サービスが稼働していません: $SERVICE_NAME"

before_commit="$(git rev-parse HEAD)"
log "更新を取得しています: $branch"
run_as_app_user git pull --ff-only origin "$branch"
after_commit="$(git rev-parse HEAD)"

if [ "$before_commit" = "$after_commit" ]; then
  log "ソースコードに更新はありません"
else
  log "ビルドしています"
  run_as_app_user cargo build --locked --release
fi

systemctl is-active --quiet "$SERVICE_NAME" || fail "サービスが稼働していません: $SERVICE_NAME"
stopped=0
restore_service() {
  if [ "$stopped" -eq 1 ]; then
    log "更新に失敗したためサービスを再起動します"
    systemctl start "$SERVICE_NAME" || true
  fi
}
trap restore_service EXIT

systemctl stop "$SERVICE_NAME"
stopped=1
backup_data
systemctl start "$SERVICE_NAME"
stopped=0
trap - EXIT

systemctl is-active --quiet "$SERVICE_NAME" || fail "サービスの起動に失敗しました: $SERVICE_NAME"
curl --insecure --fail --silent --show-error --location --max-time 15 "$HEALTH_URL" >/dev/null

log "更新が完了しました"
log "更新前: $before_commit"
log "更新後: $after_commit"
systemctl --no-pager --full status "$SERVICE_NAME" | sed -n '1,12p'
