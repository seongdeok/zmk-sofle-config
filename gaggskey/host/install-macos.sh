#!/usr/bin/env bash
# GaggsKey macOS installer — build on the Mac, install, start on login.
#
#   ./install-macos.sh                 build + install to /Applications + login item
#   ./install-macos.sh --no-build      install the already built .app
#   ./install-macos.sh --no-autostart  don't start on login
#   ./install-macos.sh --user          install to ~/Applications instead
#   ./install-macos.sh --uninstall
#
# Needs: Xcode Command Line Tools, Rust (rustup), Node.js 18+.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$HERE/apps/gaggskey-app"
APP_NAME="GaggsKey.app"
BUNDLE="$HERE/target/release/bundle/macos/$APP_NAME"
DEST_DIR="/Applications"
BIN_DIR="$HOME/.local/bin"
LABEL="dev.gaggs.gaggskey"
AGENT="$HOME/Library/LaunchAgents/$LABEL.plist"

build=1 autostart=1 uninstall=0
for a in "$@"; do
  case "$a" in
    --no-build) build=0 ;;
    --no-autostart) autostart=0 ;;
    --user) DEST_DIR="$HOME/Applications" ;;
    --uninstall) uninstall=1 ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done
DEST="$DEST_DIR/$APP_NAME"

log() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == Darwin ]] || die "macOS 전용 스크립트입니다 (Linux는 install-linux.sh)"

stop_app() {
  launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
  pkill -x gaggskey-app 2>/dev/null || true
  sleep 0.5
}

# copy/remove under DEST_DIR, escalating only if needed
in_dest() {
  if [[ -w "$DEST_DIR" ]]; then "$@"; else sudo "$@"; fi
}

if (( uninstall )); then
  log "stopping app"
  stop_app
  rm -fv "$AGENT" "$BIN_DIR/gkctl"
  for d in "/Applications/$APP_NAME" "$HOME/Applications/$APP_NAME"; do
    [[ -d $d ]] && { DEST_DIR="$(dirname "$d")"; in_dest rm -rf "$d"; echo "removed $d"; }
  done
  log "uninstalled"
  exit 0
fi

if (( build )); then
  command -v xcode-select >/dev/null && xcode-select -p >/dev/null 2>&1 \
    || die "Xcode Command Line Tools 필요: xcode-select --install"
  command -v cargo >/dev/null || die "Rust 필요: https://rustup.rs (curl ... | sh)"
  command -v npm >/dev/null || die "Node.js 필요: brew install node"

  log "building GaggsKey.app (tauri build --bundles app)"
  ( cd "$APP_DIR" && { [[ -d node_modules ]] || npm install; } && npx tauri build --bundles app )
  log "building gkctl"
  ( cd "$HERE" && cargo build --release -p gkctl )
fi

[[ -d "$BUNDLE" ]] || die "missing $BUNDLE (--no-build 없이 실행하세요)"

log "installing $DEST"
stop_app
mkdir -p "$DEST_DIR" 2>/dev/null || true
in_dest rm -rf "$DEST"
in_dest cp -R "$BUNDLE" "$DEST"
# Locally built, unsigned: clear quarantine and ad-hoc sign so Gatekeeper and
# the Bluetooth (TCC) permission prompt keep working across launches.
in_dest xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true
in_dest codesign --force --deep --sign - "$DEST" >/dev/null 2>&1 || log "warning: ad-hoc codesign failed"

log "installing gkctl to $BIN_DIR"
install -d "$BIN_DIR"
install -m755 "$HERE/target/release/gkctl" "$BIN_DIR/gkctl"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) log "note: $BIN_DIR 가 PATH에 없습니다 (~/.zshrc 에 export PATH=\"\$HOME/.local/bin:\$PATH\")";; esac

if (( autostart )); then
  log "registering login item ($AGENT)"
  mkdir -p "$(dirname "$AGENT")"
  cat >"$AGENT" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$DEST/Contents/MacOS/gaggskey-app</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
EOF
  launchctl bootstrap "gui/$(id -u)" "$AGENT" 2>/dev/null || open "$DEST"
else
  rm -f "$AGENT"
  open "$DEST"
fi

log "done — 처음 실행 시 '블루투스' 권한 요청을 허용하세요"
