#!/bin/sh
# Stage the whatis binary into this directory, then build the image.
# Run from anywhere; this script cd's into its own directory.
set -e

cd "$(dirname "$0")"

SRC=../target/release/whatis
if [ ! -x "$SRC" ]; then
    echo "error: $SRC not found. Build it first with:" >&2
    echo "    cargo build --release --bin whatis" >&2
    exit 1
fi

cp -f "$SRC" ./whatis
echo "[build.sh] staged whatis binary ($(du -h ./whatis | cut -f1))"

docker build -t "${WHATIS_IMAGE:-whatis}" .
