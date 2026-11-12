#!/usr/bin/env bash
set -euo pipefail

DRY_RUN="${DRY_RUN:-0}"
PURGE_CONFIG=0
PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
LAUNCH_AGENTS_DIR="${LAUNCH_AGENTS_DIR:-$HOME/Library/LaunchAgents}"
CONFIG_DIR="${CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/mac-hardware-volume-control}"

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --purge-config)
      PURGE_CONFIG=1
      shift
      ;;
    *)
      echo "Error: Unknown option '$1'." >&2
      exit 2
      ;;
  esac
done

run_cmd() {
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "[DRY RUN] $*"
  else
    "$@"
  fi
}

maybe_sudo() {
  local target_dir="$1"
  shift
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "[DRY RUN] $*"
    return
  fi
  if [ -w "$target_dir" ]; then
    "$@"
  else
    sudo "$@"
  fi
}

SERVICE_NAME="com.user.mac-hardware-volume-control"
PLIST_NAME="${SERVICE_NAME}.plist"
TARGET_BIN="$BIN_DIR/mac-hardware-volume-control"
PLIST_DEST="$LAUNCH_AGENTS_DIR/$PLIST_NAME"
CONFIG_DEST="$CONFIG_DIR/config.json"
USER_ID="$(id -u)"

echo "Stopping and unloading launchd service..."
if [ "${SKIP_LAUNCHCTL:-0}" -eq 1 ]; then
  echo "Skipping launchctl bootout (SKIP_LAUNCHCTL=1)."
elif [ "$DRY_RUN" -eq 1 ]; then
  run_cmd launchctl bootout "gui/$USER_ID/$SERVICE_NAME"
  run_cmd pkill -f "$TARGET_BIN"
else
  if launchctl print "gui/$USER_ID/$SERVICE_NAME" &>/dev/null; then
    run_cmd launchctl bootout "gui/$USER_ID/$SERVICE_NAME"
  fi
  if pgrep -f "$TARGET_BIN" &>/dev/null; then
    run_cmd pkill -f "$TARGET_BIN"
  fi
fi

if [ -f "$PLIST_DEST" ] || [ "$DRY_RUN" -eq 1 ]; then
  echo "Removing plist from $PLIST_DEST..."
  run_cmd rm -f "$PLIST_DEST"
fi

if [ -f "$TARGET_BIN" ] || [ "$DRY_RUN" -eq 1 ]; then
  echo "Removing binary from $TARGET_BIN..."
  maybe_sudo "$BIN_DIR" rm -f "$TARGET_BIN"
fi

if [ "$PURGE_CONFIG" -eq 1 ] && { [ -f "$CONFIG_DEST" ] || [ "$DRY_RUN" -eq 1 ]; }; then
  echo "Removing config from $CONFIG_DEST..."
  run_cmd rm -f "$CONFIG_DEST"
fi

echo "Uninstallation complete."
