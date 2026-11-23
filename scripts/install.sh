#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DRY_RUN="${DRY_RUN:-0}"
DEVICE_ID="${DEVICE_ID:-}"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-}"
CODESIGN_CERT_NAME="mac-hardware-volume-control"
SKIP_CODESIGN="${SKIP_CODESIGN:-0}"
PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
LAUNCH_AGENTS_DIR="${LAUNCH_AGENTS_DIR:-$HOME/Library/LaunchAgents}"
CONFIG_DIR="${CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/mac-hardware-volume-control}"
SERVICE_LABEL="com.user.mac-hardware-volume-control"
PLIST_TEMPLATE="$REPO_ROOT/assets/${SERVICE_LABEL}.plist.template"
CONFIG_TEMPLATE="$REPO_ROOT/assets/config.json.template"

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --device <id>       Explicitly select hardware backend ID (e.g., scarlett-gen4)
  --codesign <id>     Sign installed binary with a Keychain code-signing identity
  --dry-run           Print actions without modifying the system
  -h, --help          Show this help message
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --device)
      if [ $# -lt 2 ]; then
        echo "Error: --device requires an argument." >&2
        exit 2
      fi
      DEVICE_ID="$2"
      shift 2
      ;;
    --codesign)
      if [ $# -lt 2 ]; then
        echo "Error: --codesign requires an argument." >&2
        exit 2
      fi
      CODESIGN_IDENTITY="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Error: Unknown option '$1'." >&2
      usage >&2
      exit 2
      ;;
  esac
done

step() {
  if [ -t 1 ]; then
    printf '\n\033[1m==>\033[0m %s\n' "$1"
  else
    printf '\n==> %s\n' "$1"
  fi
}

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
  if [ -w "$target_dir" ] || { [ ! -e "$target_dir" ] && [ -w "$(dirname "$target_dir")" ]; }; then
    "$@"
  else
    sudo "$@"
  fi
}

if ! command -v cargo &> /dev/null; then
  echo "Error: Cargo/Rust toolchain not found."
  echo "Please install Rust from https://www.rust-lang.org/tools/install and try again."
  exit 1
fi

if [ ! -f "$PLIST_TEMPLATE" ]; then
  echo "Error: Plist template not found at $PLIST_TEMPLATE" >&2
  exit 1
fi

if [ ! -f "$CONFIG_TEMPLATE" ]; then
  echo "Error: Config template not found at $CONFIG_TEMPLATE" >&2
  exit 1
fi

TARGET_BIN="$BIN_DIR/mac-hardware-volume-control"
PLIST_DEST="$LAUNCH_AGENTS_DIR/${SERVICE_LABEL}.plist"
CONFIG_DEST="$CONFIG_DIR/config.json"
USER_ID="$(id -u)"

step "Building release binary..."
run_cmd cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"

BUILT_BIN="$REPO_ROOT/target/release/mac-hardware-volume-control"

if [ -z "$DEVICE_ID" ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "[DRY RUN] $BUILT_BIN --detect"
    DEVICE_ID="scarlett-gen4"
  elif [ -x "$BUILT_BIN" ]; then
    if DETECTED_LINE="$("$BUILT_BIN" --detect 2>/dev/null | head -n 1)" && [ -n "$DETECTED_LINE" ]; then
      DEVICE_ID="$(printf '%s' "$DETECTED_LINE" | cut -f1)"
      echo "  Auto-detected hardware device: $DEVICE_ID"
    else
      DEVICE_ID="scarlett-gen4"
      echo "  No supported hardware detected; defaulting to '$DEVICE_ID'."
    fi
  else
    DEVICE_ID="scarlett-gen4"
  fi
fi

step "Preparing directories and signing..."
run_cmd mkdir -p "$LAUNCH_AGENTS_DIR" "$CONFIG_DIR"
if [ ! -d "$BIN_DIR" ]; then
  maybe_sudo "$BIN_DIR" mkdir -p "$BIN_DIR"
fi

ensure_codesign_identity() {
  if [ -n "$CODESIGN_IDENTITY" ] || [ "$SKIP_CODESIGN" -eq 1 ]; then
    return 0
  fi

  if command -v security &>/dev/null && security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$CODESIGN_CERT_NAME\""; then
    echo "  Using existing code-signing certificate '$CODESIGN_CERT_NAME'."
    CODESIGN_IDENTITY="$CODESIGN_CERT_NAME"
    return 0
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    echo "[DRY RUN] create/import self-signed code-signing certificate '$CODESIGN_CERT_NAME'"
    CODESIGN_IDENTITY="$CODESIGN_CERT_NAME"
    return 0
  fi

  if command -v openssl &>/dev/null && command -v security &>/dev/null; then
    echo "  Creating self-signed code-signing certificate '$CODESIGN_CERT_NAME' in login keychain..."
    local tmp_dir p12_pass keychain
    tmp_dir="$(mktemp -d)"
    p12_pass="$(uuidgen)"
    keychain="$HOME/Library/Keychains/login.keychain-db"

    cat > "$tmp_dir/openssl.cnf" <<EOF
[req]
distinguished_name = req_distinguished_name
x509_extensions = codesign
prompt = no

[req_distinguished_name]
CN = $CODESIGN_CERT_NAME

[codesign]
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
basicConstraints = critical, CA:true
EOF

    if openssl req -x509 -newkey rsa:2048 -nodes \
        -keyout "$tmp_dir/key.pem" -out "$tmp_dir/cert.pem" \
        -days 3650 -config "$tmp_dir/openssl.cnf" >/dev/null 2>&1 \
      && openssl pkcs12 -export -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1 \
        -out "$tmp_dir/cert.p12" \
        -inkey "$tmp_dir/key.pem" -in "$tmp_dir/cert.pem" \
        -passout "pass:$p12_pass" >/dev/null 2>&1 \
      && security import "$tmp_dir/cert.p12" -k "$keychain" -P "$p12_pass" -T /usr/bin/codesign \
      && security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$tmp_dir/cert.pem" \
      && security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$CODESIGN_CERT_NAME\""; then
      rm -rf "$tmp_dir"
      CODESIGN_IDENTITY="$CODESIGN_CERT_NAME"
      return 0
    fi
    rm -rf "$tmp_dir"
  fi

  echo ""
  echo "Could not automatically create/trust code-signing certificate '$CODESIGN_CERT_NAME'."
  echo "To preserve privacy permissions across rebuilds, create it once in Keychain Access:"
  echo "  1. Open Keychain Access -> Certificate Assistant -> Create a Certificate..."
  echo "  2. Name: $CODESIGN_CERT_NAME"
  echo "  3. Identity Type: Self-Signed Root"
  echo "  4. Certificate Type: Code Signing"
  if [ -t 0 ]; then
    read -r -p "Press Enter once created to continue (or Ctrl+C to abort)... "
    if command -v security &>/dev/null && security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$CODESIGN_CERT_NAME\""; then
      CODESIGN_IDENTITY="$CODESIGN_CERT_NAME"
      return 0
    fi
  fi
  echo "Warning: '$CODESIGN_CERT_NAME' certificate not found; proceeding with ad-hoc signature." >&2
}

ensure_codesign_identity

if [ -n "$CODESIGN_IDENTITY" ]; then
  echo "  Signing release binary with '$CODESIGN_IDENTITY'..."
  run_cmd codesign --force --sign "$CODESIGN_IDENTITY" "$BUILT_BIN"
fi

step "Installing binary, config, and LaunchAgent..."
echo "  Binary: $TARGET_BIN"
maybe_sudo "$BIN_DIR" install -m 755 "$BUILT_BIN" "$TARGET_BIN"

echo "  Config: $CONFIG_DEST (device: $DEVICE_ID)"
if [ "$DRY_RUN" -eq 1 ]; then
  echo "[DRY RUN] render $CONFIG_TEMPLATE -> $CONFIG_DEST (device_id=$DEVICE_ID)"
else
  sed "s|__DEVICE_ID__|$DEVICE_ID|g" "$CONFIG_TEMPLATE" > "$CONFIG_DEST"
fi

echo "  Plist:  $PLIST_DEST"
if [ "$DRY_RUN" -eq 1 ]; then
  echo "[DRY RUN] render $PLIST_TEMPLATE -> $PLIST_DEST"
else
  sed -e "s|__SERVICE_LABEL__|$SERVICE_LABEL|g" \
      -e "s|__BINARY_PATH__|$TARGET_BIN|g" \
      "$PLIST_TEMPLATE" > "$PLIST_DEST"
  plutil -lint "$PLIST_DEST" > /dev/null
fi

if [ "${SKIP_LAUNCHCTL:-0}" -eq 1 ]; then
  echo "Skipping launchctl bootstrap (SKIP_LAUNCHCTL=1)."
else
  step "Loading launchd service..."
  echo "  Service: gui/$USER_ID/$SERVICE_LABEL"
  run_cmd launchctl bootout "gui/$USER_ID" "$PLIST_DEST" 2>/dev/null || true
  run_cmd launchctl bootstrap "gui/$USER_ID" "$PLIST_DEST"
fi

echo ""
echo "Installation complete (device: $DEVICE_ID)."
