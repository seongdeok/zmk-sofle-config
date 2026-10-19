#!/usr/bin/env bash
# GaggsKey Linux installer (per-user, no root except for the udev rule).
#
#   ./install-linux.sh                 build + install + autostart
#   ./install-linux.sh --no-build      install existing target/release binaries
#   ./install-linux.sh --no-autostart  don't start on login
#   ./install-linux.sh --uninstall
#
# Installs:
#   ~/.local/bin/{gaggskey-app,gkctl}
#   ~/.local/share/applications/gaggskey.desktop
#   ~/.local/share/icons/hicolor/<size>/apps/gaggskey.png
#   ~/.config/autostart/gaggskey.desktop          (unless --no-autostart)
#   /etc/udev/rules.d/70-gaggskey.rules            (via pkexec/sudo, if missing)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$HERE/apps/gaggskey-app"
BIN_DIR="$HOME/.local/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"
DESKTOP="$DATA/applications/gaggskey.desktop"
AUTOSTART="$CONF/autostart/gaggskey.desktop"
ICONS="$DATA/icons/hicolor"
UDEV_SRC="$HERE/udev/70-gaggskey.rules"
UDEV_DST="/etc/udev/rules.d/70-gaggskey.rules"
UDEV_OLD="/etc/udev/rules.d/70-sofle-optionsplus.rules"

build=1 autostart=1 uninstall=0
for a in "$@"; do
  case "$a" in
    --no-build) build=0 ;;
    --no-autostart) autostart=0 ;;
    --uninstall) uninstall=1 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

log() { printf '\e[1;33m==>\e[0m %s\n' "$*"; }

as_root() {
  if [[ $EUID -eq 0 ]]; then "$@"
  elif command -v pkexec >/dev/null && [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then pkexec "$@"
  else sudo "$@"; fi
}

refresh_caches() {
  command -v update-desktop-database >/dev/null && update-desktop-database -q "$DATA/applications" || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$ICONS" 2>/dev/null || true
}

if (( uninstall )); then
  log "stopping running app"
  pkill -x gaggskey-app 2>/dev/null || true
  rm -fv "$BIN_DIR/gaggskey-app" "$BIN_DIR/gkctl" "$DESKTOP" "$AUTOSTART"
  rm -fv "$ICONS"/*/apps/gaggskey.png
  refresh_caches
  if [[ -f $UDEV_DST ]]; then
    log "removing udev rule (needs root)"
    as_root sh -c "rm -f '$UDEV_DST' && udevadm control --reload"
  fi
  log "uninstalled"
  exit 0
fi

if (( build )); then
  log "building app (tauri build --no-bundle)"
  ( cd "$APP_DIR" && { [[ -d node_modules ]] || npm install; } && npx tauri build --no-bundle )
  log "building gkctl"
  ( cd "$HERE" && cargo build --release -p gkctl )
fi

for b in gaggskey-app gkctl; do
  [[ -x "$HERE/target/release/$b" ]] || { echo "missing $HERE/target/release/$b (run without --no-build)" >&2; exit 1; }
done

log "installing binaries to $BIN_DIR"
was_running=0
pgrep -x gaggskey-app >/dev/null && { was_running=1; pkill -x gaggskey-app; sleep 0.5; }
install -Dm755 "$HERE/target/release/gaggskey-app" "$BIN_DIR/gaggskey-app"
install -Dm755 "$HERE/target/release/gkctl" "$BIN_DIR/gkctl"

log "installing icons"
icons_src="$APP_DIR/src-tauri/icons"
install -Dm644 "$icons_src/32x32.png"        "$ICONS/32x32/apps/gaggskey.png"
install -Dm644 "$icons_src/64x64.png"        "$ICONS/64x64/apps/gaggskey.png"
install -Dm644 "$icons_src/128x128.png"      "$ICONS/128x128/apps/gaggskey.png"
install -Dm644 "$icons_src/128x128@2x.png"   "$ICONS/256x256/apps/gaggskey.png"

log "installing desktop entry"
mkdir -p "$(dirname "$DESKTOP")"
cat >"$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=GaggsKey
GenericName=Keyboard Manager
Comment=ZMK keyboard status, layers and keymap
Exec=$BIN_DIR/gaggskey-app
Icon=gaggskey
Terminal=false
Categories=Settings;HardwareSettings;
Keywords=keyboard;zmk;sofle;layer;battery;
StartupWMClass=gaggskey-app
EOF

if (( autostart )); then
  log "enabling autostart ($AUTOSTART)"
  mkdir -p "$(dirname "$AUTOSTART")"
  cp "$DESKTOP" "$AUTOSTART"
  echo "X-GNOME-Autostart-enabled=true" >>"$AUTOSTART"
else
  rm -f "$AUTOSTART"
fi
refresh_caches

if [[ -f $UDEV_DST ]] && cmp -s "$UDEV_SRC" "$UDEV_DST" && [[ ! -f $UDEV_OLD ]]; then
  log "udev rule already installed"
else
  log "installing udev rule for USB access (needs root)"
  as_root sh -c "install -Dm644 '$UDEV_SRC' '$UDEV_DST' && rm -f '$UDEV_OLD' \
                 && udevadm control --reload && udevadm trigger --subsystem-match=hidraw"
fi

case ":$PATH:" in *":$BIN_DIR:"*) ;; *) log "note: $BIN_DIR is not in PATH (gkctl)";; esac

if (( was_running )) || (( autostart )); then
  log "starting gaggskey-app"
  setsid "$BIN_DIR/gaggskey-app" >/dev/null 2>&1 < /dev/null &
fi
log "done — 앱 메뉴에서 'GaggsKey'로 실행할 수 있습니다"
