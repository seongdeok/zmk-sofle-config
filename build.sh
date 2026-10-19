#!/usr/bin/env bash
# Local docker build for every keyboard in this repo.
#
#   ./build.sh                    # all keyboards, both halves
#   ./build.sh corne              # corne left (central, GaggsKey + Studio) + right
#   ./build.sh kyria left         # one half
#   ./build.sh sofle right
#   ./build.sh reset              # settings_reset
#
# Keymaps are regenerated first (keymap/gen.py): config/corne.keymap is the base,
# keymap/<board>.toml holds each other keyboard's differences.
set -euo pipefail
cd "$(dirname "$0")"
IMAGE=${ZMK_IMAGE:-zmkfirmware/zmk-dev-arm:3.5}
KB=${1:-all}
SIDE=${2:-all}

python3 keymap/gen.py
mkdir -p firmware

run() {
  docker run --rm --workdir /workdir --volume "$PWD:/workdir:Z" "$IMAGE" sh -lc "
    set -eu
    git config --global --add safe.directory '*'
    export ZEPHYR_BASE=/workdir/zephyr
    export Zephyr_DIR=/workdir/zephyr/share/zephyr-package
    export CMAKE_PREFIX_PATH=/workdir/zephyr/share/zephyr-package
    $1"
}

# build <build-dir> <shield> <artifact> [central]
build() {
  local dir=$1 shield=$2 out=$3 extra=""
  if [[ ${4:-} == central ]]; then
    extra="-DZMK_EXTRA_MODULES=/workdir/gaggskey/firmware \
           -DEXTRA_CONF_FILE=/workdir/gaggskey/firmware/gaggskey_central.conf \
           -DSNIPPET=studio-rpc-usb-uart"
  fi
  echo "==> $out ($shield)"
  run "west build -p always /workdir/zmk/app --build-dir /workdir/build/$dir --board nice_nano_v2 -- \
         -DSHIELD=\"$shield\" -DZMK_CONFIG=/workdir/config $extra \
       && cp /workdir/build/$dir/zephyr/zmk.uf2 /workdir/firmware/$out.uf2"
}

want() { [[ $KB == all || $KB == "$1" ]] && [[ $SIDE == all || $SIDE == "$2" ]]; }

want sofle left  && build sofle-left  "sofle_left nice_oled"  nice_sofle_left_oled_gaggskey central
want sofle right && build sofle-right "sofle_right nice_oled" nice_sofle_right_oled
want corne left  && build corne-left  corne_left              corne_left_gaggskey central
want corne right && build corne-right corne_right             corne_right
want kyria left  && build kyria-left  gaggs_kyria_left        gaggs_kyria_left_gaggskey central
want kyria right && build kyria-right gaggs_kyria_right       gaggs_kyria_right
[[ $KB == reset ]] && build reset settings_reset nice_settings_reset
ls -la firmware/*.uf2 | grep "$(date +%b\ %e)" || true
