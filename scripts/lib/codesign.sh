#!/usr/bin/env bash
# Self-signed code-signing certificate helper for macOS TCC persistence.

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
