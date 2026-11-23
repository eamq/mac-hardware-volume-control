#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=scripts/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

DRY_RUN="${DRY_RUN:-0}"
SKIP_CODESIGN="${SKIP_CODESIGN:-0}"
CODESIGN_CERT_NAME="mac-hardware-volume-control"
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
    *)
      echo "Error: Unknown option '$1'." >&2
      exit 2
      ;;
  esac
done

SERVICE_NAME="com.user.mac-hardware-volume-control"
PLIST_NAME="${SERVICE_NAME}.plist"
TARGET_BIN="$BIN_DIR/mac-hardware-volume-control"
PLIST_DEST="$LAUNCH_AGENTS_DIR/$PLIST_NAME"
CONFIG_DEST="$CONFIG_DIR/config.json"
USER_ID="$(id -u)"

step "Stopping and unloading launchd service..."
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

step "Removing installed files..."
if [ -f "$PLIST_DEST" ] || [ "$DRY_RUN" -eq 1 ]; then
  echo "  Plist:  $PLIST_DEST"
  run_cmd rm -f "$PLIST_DEST"
fi

if [ -f "$TARGET_BIN" ] || [ "$DRY_RUN" -eq 1 ]; then
  echo "  Binary: $TARGET_BIN"
  maybe_sudo "$BIN_DIR" rm -f "$TARGET_BIN"
fi

if [ -f "$CONFIG_DEST" ] || [ "$DRY_RUN" -eq 1 ]; then
  echo "  Config: $CONFIG_DEST"
  run_cmd rm -f "$CONFIG_DEST"
  if [ "$DRY_RUN" -eq 0 ] && [ -d "$CONFIG_DIR" ]; then
    rmdir "$CONFIG_DIR" 2>/dev/null || true
  fi
fi

KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
if [ "$SKIP_CODESIGN" -eq 1 ]; then
  echo "Skipping keychain certificate removal (SKIP_CODESIGN=1)."
elif [ "$DRY_RUN" -eq 1 ]; then
  step "Removing code-signing certificate '$CODESIGN_CERT_NAME' from keychain..."
  run_cmd security delete-identity -t -c "$CODESIGN_CERT_NAME" "$KEYCHAIN"
elif command -v security &>/dev/null && security find-certificate -c "$CODESIGN_CERT_NAME" "$KEYCHAIN" &>/dev/null; then
  step "Removing code-signing certificate '$CODESIGN_CERT_NAME' from keychain..."
  run_cmd security delete-identity -t -c "$CODESIGN_CERT_NAME" "$KEYCHAIN"
fi

echo ""
echo "Uninstallation complete."
if [ -t 1 ]; then
  printf '\033[1;33mNote:\033[0m To remove the leftover permission entry, open System Settings -> Privacy & Security -> Device Control and Data Access (Accessibility on macOS 26 and earlier) and remove mac-hardware-volume-control.\n'
else
  echo "Note: To remove the leftover permission entry, open System Settings -> Privacy & Security -> Device Control and Data Access (Accessibility on macOS 26 and earlier) and remove mac-hardware-volume-control."
fi
