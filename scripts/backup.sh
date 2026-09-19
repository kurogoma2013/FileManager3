#!/usr/bin/env bash
set -euo pipefail

# FileManager3 のバックアップスクリプト（Ubuntu + systemd 用）
# PostgreSQL の論理ダンプとストレージの tar.gz を同一ディレクトリに保存し、保持日数を過ぎた世代を削除する。
# 例: sudo FILEMANAGER_BACKUP_KEEP_DAYS=14 ./scripts/backup.sh
# 定期実行は scripts/systemd/filemanager-backup.timer を参照。

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_USER="${FILEMANAGER_RUN_USER:-filemanager}"
DATA_DIR="${FILEMANAGER_DATA_DIR:-/var/lib/filemanager/data}"
STORAGE_DIR="${FILEMANAGER_STORAGE_DIR:-$DATA_DIR/storage}"
BACKUP_DIR="${FILEMANAGER_BACKUP_DIR:-/var/backups/filemanager}"
DB_NAME="${FILEMANAGER_DB_NAME:-filemanager}"
KEEP_DAYS="${FILEMANAGER_BACKUP_KEEP_DAYS:-14}"

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

require_command find
require_command install
require_command pg_dump
require_command sha256sum
require_command tar
if [ "$(id -u)" -eq 0 ]; then
  require_command runuser
  id "$RUN_USER" >/dev/null 2>&1 || fail "実行ユーザーが見つかりません: $RUN_USER"
fi
[ -d "$STORAGE_DIR" ] || fail "ストレージが見つかりません: $STORAGE_DIR"
case "$KEEP_DAYS" in
  ''|*[!0-9]*) fail "FILEMANAGER_BACKUP_KEEP_DAYS は0以上の整数で指定してください: $KEEP_DAYS" ;;
esac

timestamp="$(date '+%Y%m%d-%H%M%S')"
install -d -m 750 "$BACKUP_DIR"
backup_root="$BACKUP_DIR/filemanager-${timestamp}"
work_root="${backup_root}.partial"
rm -rf "$work_root"
install -d -m 750 "$work_root"
cleanup_partial() {
  if [ -d "$work_root" ]; then
    log "バックアップに失敗したため未完成のディレクトリを削除します: $work_root"
    rm -rf "$work_root"
  fi
}
trap cleanup_partial EXIT

backup_file="$work_root/database.dump"
run_as_app_user pg_dump --format=custom --file="$backup_file" "$DB_NAME"
storage_archive="$work_root/storage.tar.gz"
tar -C "$STORAGE_DIR" -czf "$storage_archive" .
{
  printf 'バックアップ日時: %s\n' "$timestamp"
  printf 'コミット: %s\n' "$(run_as_app_user git -C "$ROOT_DIR" rev-parse HEAD 2>/dev/null || echo '不明')"
  printf 'データベース: %s\n' "$DB_NAME"
  printf 'ストレージ: %s\n' "$STORAGE_DIR"
} > "$work_root/metadata.txt"
(cd "$work_root" && sha256sum database.dump storage.tar.gz > manifest.sha256)
mv "$work_root" "$backup_root"
trap - EXIT
log "データベースとストレージをバックアップしました: $backup_root"

if [ "$KEEP_DAYS" -gt 0 ]; then
  find "$BACKUP_DIR" -mindepth 1 -maxdepth 1 -type d -name 'filemanager-*' ! -name '*.partial' \
    -mtime "+$((KEEP_DAYS - 1))" -print0 | while IFS= read -r -d '' old; do
      log "保持期間（${KEEP_DAYS}日）を過ぎたバックアップを削除します: $old"
      rm -rf "$old"
    done
fi
find "$BACKUP_DIR" -mindepth 1 -maxdepth 1 -type d -name 'filemanager-*.partial' -mmin +180 -exec rm -rf {} +

log "バックアップ世代: $(find "$BACKUP_DIR" -mindepth 1 -maxdepth 1 -type d -name 'filemanager-*' ! -name '*.partial' | wc -l | tr -d ' ') 件"
df -h "$BACKUP_DIR" | sed -n '1,2p'
