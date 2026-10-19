#!/usr/bin/env bash
# Moved: the repo-root build.sh builds every keyboard. Kept for old muscle memory.
#   ./gaggskey/build.sh [left|right|all]  ==  ./build.sh sofle [left|right|all]
exec "$(dirname "$0")/../build.sh" sofle "${1:-all}"
