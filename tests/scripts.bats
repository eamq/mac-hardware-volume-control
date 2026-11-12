#!/usr/bin/env bats

setup() {
  TEST_TMP="$(mktemp -d)"
  export PREFIX="$TEST_TMP/prefix"
  export LAUNCH_AGENTS_DIR="$TEST_TMP/LaunchAgents"
  export CONFIG_DIR="$TEST_TMP/config"
  export SKIP_LAUNCHCTL=1

  BACKUP_BIN=""
  if [ -e target/release/mac-hardware-volume-control ]; then
    BACKUP_BIN="$TEST_TMP/orig-bin"
    cp target/release/mac-hardware-volume-control "$BACKUP_BIN"
  fi
}

teardown() {
  if [ -n "${BACKUP_BIN:-}" ] && [ -f "$BACKUP_BIN" ]; then
    mkdir -p target/release
    mv "$BACKUP_BIN" target/release/mac-hardware-volume-control
  fi
  rm -rf "$TEST_TMP"
}

@test "install.sh runs cleanly in dry-run mode" {
  DRY_RUN=1 run ./scripts/install.sh --dry-run
  [ "$status" -eq 0 ]
  [[ "$output" =~ "Building release binary..." ]]
  [[ "$output" =~ "[DRY RUN] cargo build --release" ]]
  [[ "$output" =~ "assets/com.user.mac-hardware-volume-control.plist.template" ]]
}

@test "uninstall.sh runs cleanly in dry-run mode" {
  DRY_RUN=1 run ./scripts/uninstall.sh --dry-run
  [ "$status" -eq 0 ]
  [[ "$output" =~ "Stopping and unloading launchd service..." ]]
  [[ "$output" =~ "[DRY RUN]" ]]
}

@test "install.sh fails gracefully when cargo is missing" {
  DRY_RUN=1 PATH="/usr/bin:/bin" run ./scripts/install.sh --dry-run
  [ "$status" -eq 1 ]
  [[ "$output" =~ "Error: Cargo/Rust toolchain not found." ]]
}

@test "install.sh and uninstall.sh work in temp prefix without sudo" {
  mkdir -p target/release "$PREFIX/bin"
  printf '#!/usr/bin/env bash\necho "scarlett-gen4\tScarlett (Gen 4)\tScarlett 18i20 4th Gen"\n' > target/release/mac-hardware-volume-control
  chmod +x target/release/mac-hardware-volume-control

  mkdir -p "$TEST_TMP/bin"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$TEST_TMP/bin/cargo"
  chmod +x "$TEST_TMP/bin/cargo"

  PATH="$TEST_TMP/bin:$PATH" run ./scripts/install.sh
  [ "$status" -eq 0 ]
  [ -x "$PREFIX/bin/mac-hardware-volume-control" ]
  [ -f "$LAUNCH_AGENTS_DIR/com.user.mac-hardware-volume-control.plist" ]
  [ -f "$CONFIG_DIR/config.json" ]
  [[ "$(cat "$CONFIG_DIR/config.json")" =~ "scarlett-gen4" ]]
  plutil -lint "$LAUNCH_AGENTS_DIR/com.user.mac-hardware-volume-control.plist"

  run ./scripts/uninstall.sh --purge-config
  [ "$status" -eq 0 ]
  [ ! -e "$PREFIX/bin/mac-hardware-volume-control" ]
  [ ! -e "$LAUNCH_AGENTS_DIR/com.user.mac-hardware-volume-control.plist" ]
  [ ! -e "$CONFIG_DIR/config.json" ]
}
