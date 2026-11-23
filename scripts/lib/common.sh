#!/usr/bin/env bash
# Shared output, dry-run, and privilege helpers for install/uninstall scripts.

step() {
  if [ -t 1 ]; then
    printf '\n\033[1m==>\033[0m %s\n' "$1"
  else
    printf '\n==> %s\n' "$1"
  fi
}

run_cmd() {
  if [ "${DRY_RUN:-0}" -eq 1 ]; then
    echo "[DRY RUN] $*"
  else
    "$@"
  fi
}

maybe_sudo() {
  local target_dir="$1"
  shift
  if [ "${DRY_RUN:-0}" -eq 1 ]; then
    echo "[DRY RUN] $*"
    return
  fi
  if [ -w "$target_dir" ] || { [ ! -e "$target_dir" ] && [ -w "$(dirname "$target_dir")" ]; }; then
    "$@"
  else
    sudo "$@"
  fi
}
