#!/usr/bin/env bash
# Local docker build of Sofle firmware with the Options+ module.
#   ./optionsplus/build.sh          # left (central, with Options+) + right
#   ./optionsplus/build.sh left     # left only
set -euo pipefail
cd "$(dirname "$0")/.."
IMAGE=${ZMK_IMAGE:-zmkfirmware/zmk-dev-arm:3.5}
WHICH=${1:-all}

run() {
  docker run --rm --workdir /workdir --volume "$PWD:/workdir:Z" "$IMAGE" sh -lc "
    set -eu
    git config --global --add safe.directory '*'
    export ZEPHYR_BASE=/workdir/zephyr
    export Zephyr_DIR=/workdir/zephyr/share/zephyr-package
    export CMAKE_PREFIX_PATH=/workdir/zephyr/share/zephyr-package
    $1"
}

if [[ $WHICH == all || $WHICH == left ]]; then
  run 'west build -p always /workdir/zmk/app --build-dir /workdir/build-sofle-left --board nice_nano_v2 -- \
        -DSHIELD="sofle_left nice_oled" -DZMK_CONFIG=/workdir/config \
        -DZMK_EXTRA_MODULES=/workdir/optionsplus/firmware \
        -DEXTRA_CONF_FILE=/workdir/optionsplus/firmware/optionsplus_central.conf \
        -DSNIPPET=studio-rpc-usb-uart \
     && cp /workdir/build-sofle-left/zephyr/zmk.uf2 /workdir/firmware/nice_sofle_left_oled_optionsplus.uf2'
fi
if [[ $WHICH == all || $WHICH == right ]]; then
  run 'west build /workdir/zmk/app --build-dir /workdir/build-sofle-right --board nice_nano_v2 -- \
        -DSHIELD="sofle_right nice_oled" -DZMK_CONFIG=/workdir/config \
     && cp /workdir/build-sofle-right/zephyr/zmk.uf2 /workdir/firmware/nice_sofle_right_oled.uf2'
fi
ls -la firmware/*.uf2
