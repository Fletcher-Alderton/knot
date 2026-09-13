#!/usr/bin/env bash
set -Eeuo pipefail

usage() {
  cat <<'EOF'
Build the current Knot working tree, install the macOS app, and launch it.

Usage: scripts/install-macos.sh [options]

Options:
  --mode MODE         no-ai, remote, local, or both (default: both)
  --install-dir DIR   destination directory (default: $HOME/Applications)
  --no-launch         install without launching Knot
  --skip-deps         skip pnpm install --frozen-lockfile
  -h, --help          show this help

Environment equivalents:
  KNOT_AI_MODE, KNOT_INSTALL_DIR, KNOT_TARGET_DIR
EOF
}

mode="${KNOT_AI_MODE:-both}"
install_dir="${KNOT_INSTALL_DIR:-$HOME/Applications}"
launch=1
install_deps=1

while (($#)); do
  case "$1" in
    --mode)
      [[ $# -ge 2 ]] || { echo "--mode requires a value" >&2; exit 2; }
      mode="$2"
      shift 2
      ;;
    --install-dir)
      [[ $# -ge 2 ]] || { echo "--install-dir requires a value" >&2; exit 2; }
      install_dir="$2"
      shift 2
      ;;
    --no-launch)
      launch=0
      shift
      ;;
    --skip-deps)
      install_deps=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || {
  echo "This installer supports macOS only." >&2
  exit 1
}

for command in cargo pnpm codesign ditto open python3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Required command not found: $command" >&2
    exit 1
  }
done

case "$mode" in
  no-ai) features=() ;;
  remote) features=(--features remote-ai) ;;
  local) features=(--features local-ai) ;;
  both) features=(--features remote-ai,local-ai) ;;
  *)
    echo "Invalid mode '$mode'; expected no-ai, remote, local, or both." >&2
    exit 2
    ;;
esac

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
app_dir="$repo_root/apps/desktop"
target_dir="${KNOT_TARGET_DIR:-$repo_root/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$repo_root/$target_dir"
fi
bundle="$target_dir/release/bundle/macos/Knot.app"
destination="$install_dir/Knot.app"
staging="$install_dir/.Knot.app.install.$$"

cleanup() {
  rm -rf -- "$staging"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

revision="$(git -C "$repo_root" rev-parse --short HEAD 2>/dev/null || printf unknown)"
if [[ -n "$(git -C "$repo_root" status --porcelain 2>/dev/null || true)" ]]; then
  revision="$revision + local changes"
fi

echo "Building Knot from $revision (AI mode: $mode)"
if ((install_deps)); then
  pnpm --dir "$app_dir" install --frozen-lockfile
fi

(
  cd -- "$app_dir"
  CARGO_TARGET_DIR="$target_dir" \
    MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-10.15}" \
    cargo tauri build --bundles app --ci "${features[@]}"
)

[[ -d "$bundle" ]] || {
  echo "Build completed but app bundle was not found: $bundle" >&2
  exit 1
}

bundle_identifier() {
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$1/Contents/Info.plist" 2>/dev/null
}

expected_bundle_id="dev.knot.desktop"
[[ "$(bundle_identifier "$bundle")" == "$expected_bundle_id" ]] || {
  echo "Built app has an unexpected or missing bundle identifier." >&2
  exit 1
}
bundle_executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle/Contents/Info.plist" 2>/dev/null || true)"
[[ -n "$bundle_executable" && -x "$bundle/Contents/MacOS/$bundle_executable" ]] || {
  echo "Built app has an invalid executable declaration." >&2
  exit 1
}

mkdir -p -- "$install_dir"
if [[ -L "$destination" ]]; then
  echo "Refusing to replace symlink: $destination" >&2
  exit 1
fi
if [[ -e "$destination" ]]; then
  [[ -d "$destination" && "$(bundle_identifier "$destination")" == "$expected_bundle_id" ]] || {
    echo "Refusing to replace unrelated path: $destination" >&2
    exit 1
  }
fi
rm -rf -- "$staging"
ditto "$bundle" "$staging"
codesign --force --deep --sign - "$staging"
codesign --verify --deep --strict "$staging"

installed_knot_pids() {
  local pid command_path expected
  expected="$destination/Contents/MacOS/$bundle_executable"
  for pid in $(pgrep -x "$bundle_executable" 2>/dev/null || true); do
    command_path="$(ps -ww -p "$pid" -o command= 2>/dev/null || true)"
    [[ "$command_path" == "$expected" || "$command_path" == "$expected "* ]] && printf '%s\n' "$pid"
  done
}

if [[ -n "$(installed_knot_pids)" ]]; then
  for pid in $(installed_knot_pids); do
    kill -TERM "$pid" 2>/dev/null || true
  done
  for _ in {1..40}; do
    [[ -z "$(installed_knot_pids)" ]] && break
    sleep 0.25
  done
  if [[ -n "$(installed_knot_pids)" ]]; then
    echo "The installed Knot app could not be stopped safely." >&2
    exit 1
  fi
fi

if [[ -e "$destination" ]]; then
  python3 - "$staging" "$destination" <<'PY'
import ctypes
import os
import sys

source, destination = map(os.fsencode, sys.argv[1:3])
libc = ctypes.CDLL(None, use_errno=True)
renamex_np = libc.renamex_np
renamex_np.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
renamex_np.restype = ctypes.c_int
RENAME_SWAP = 0x00000002
if renamex_np(source, destination, RENAME_SWAP) != 0:
    error = ctypes.get_errno()
    raise OSError(error, os.strerror(error))
PY
  rm -rf -- "$staging"
else
  mv -- "$staging" "$destination"
fi
trap - EXIT INT TERM

echo "Installed Knot at $destination"
if ((launch)); then
  open -- "$destination"
  echo "Launched Knot"
fi
