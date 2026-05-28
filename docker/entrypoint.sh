#!/bin/sh
set -e

SAMPLES_SRC=/samples
TMPFS_SIZE="${WHATIS_TMPFS_SIZE:-2G}"
PASSWORD=infected
DEBUG="${WHATIS_DEBUG:-0}"

# ---------------------------------------------------------------------------
# 1. Pick a tmpfs-backed extraction root. The host's AV will scan and
#    quarantine anything decrypted into the regular overlay filesystem,
#    so the extracted samples MUST live in memory.
# ---------------------------------------------------------------------------
is_tmpfs() {
    # Returns 0 iff $1 is on a tmpfs (or ramfs) mount.
    case "$(stat -f -c '%T' "$1" 2>/dev/null)" in
        tmpfs|ramfs) return 0 ;;
        *)           return 1 ;;
    esac
}

EXTRACT_ROOT=""

# Option A: caller passed --tmpfs /tmp (or similar): /tmp is already tmpfs.
if is_tmpfs /tmp; then
    EXTRACT_ROOT=/tmp/samples
    echo "[entrypoint] /tmp is tmpfs — using it"
# Option B: we have CAP_SYS_ADMIN and can mount tmpfs ourselves.
elif mount -t tmpfs -o "size=${TMPFS_SIZE}" tmpfs /tmp 2>/dev/null; then
    EXTRACT_ROOT=/tmp/samples
    echo "[entrypoint] mounted tmpfs (size=${TMPFS_SIZE}) at /tmp"
# Option C: /dev/shm is tmpfs by default in Docker (cap-free fallback).
elif is_tmpfs /dev/shm; then
    EXTRACT_ROOT=/dev/shm/samples
    mkdir -p /tmp
    ln -snf /dev/shm/samples /tmp/samples
    SHM_SIZE=$(df -BM /dev/shm 2>/dev/null | awk 'NR==2 {print $2}')
    echo "[entrypoint] using /dev/shm tmpfs (size=${SHM_SIZE:-default})"
    echo "[entrypoint]   /tmp/samples is symlinked to /dev/shm/samples"
    echo "[entrypoint]   to enlarge, run with --shm-size=2g"
else
    cat >&2 <<EOF
[entrypoint] FATAL: no tmpfs available for extraction.

The decrypted samples must live in memory, otherwise the host's AV will
quarantine them on the Docker overlay filesystem.  Re-run the container
with ONE of:

    docker run ... --tmpfs /tmp:rw,size=${TMPFS_SIZE}   # cleanest
    docker run ... --cap-add SYS_ADMIN                  # let entrypoint mount
    docker run ... --shm-size=${TMPFS_SIZE}             # use /dev/shm fallback
EOF
    exit 1
fi

mkdir -p "$EXTRACT_ROOT"

# ---------------------------------------------------------------------------
# 2. Decrypt and extract each archive in /samples/ into its own subdirectory.
#    We use 7z because the archives use WinZip AES (compression method 99),
#    which Debian's stock unzip can't handle.  7z also auto-detects archive
#    format by content, so inner files renamed to e.g. *.zip_x are fine.
# ---------------------------------------------------------------------------
if [ "$DEBUG" -gt 0 ]; then
    Q=""           # show 7z output
else
    Q="-bso0 -bse0"
fi

extract_with_7z() {
    7z x "-p${PASSWORD}" -o"$2" -y -bd $Q "$1"
}

is_archive() {
    7z l "-p${PASSWORD}" "$1" >/dev/null 2>&1
}

found=0
for outer in "$SAMPLES_SRC"/*; do
    [ -f "$outer" ] || continue
    found=1
    base=$(basename "$outer")
    name="${base%.*}"
    dest="$EXTRACT_ROOT/$name"
    mkdir -p "$dest"

    staging=$(mktemp -d -p "$EXTRACT_ROOT" "${name}.outer.XXXXXX")
    echo "[entrypoint] outer:  $outer -> $staging"
    if ! extract_with_7z "$outer" "$staging"; then
        echo "[entrypoint] failed to extract outer archive $outer" >&2
        rm -rf "$staging"
        continue
    fi

    inner_found=0
    for inner in $(find "$staging" -maxdepth 4 -type f); do
        if is_archive "$inner"; then
            echo "[entrypoint] inner:  $(basename "$inner") -> $dest"
            if extract_with_7z "$inner" "$dest"; then
                inner_found=1
            else
                echo "[entrypoint] failed to extract inner $inner" >&2
            fi
        fi
    done

    if [ "$inner_found" -eq 0 ]; then
        echo "[entrypoint] no inner archive detected; treating outer as single-layer"
        find "$staging" -mindepth 1 -maxdepth 1 -exec mv {} "$dest/" \;
    fi
    rm -rf "$staging"
done

if [ "$found" -eq 0 ]; then
    echo "[entrypoint] no archive files in $SAMPLES_SRC; nothing to extract."
else
    echo "[entrypoint] samples ready under $EXTRACT_ROOT (also at /tmp/samples):"
    # Verify extracted files are still present — if AV swept them despite
    # tmpfs (shouldn't happen, but worth catching), report it clearly.
    for d in "$EXTRACT_ROOT"/*; do
        [ -d "$d" ] || continue
        n=$(find "$d" -type f | wc -l)
        size=$(du -sh "$d" 2>/dev/null | cut -f1)
        printf "  %-40s  %4d files  %s\n" "$(basename "$d")/" "$n" "$size"
        if [ "$n" -eq 0 ]; then
            echo "    WARN: $d is empty — extraction failed or files were removed" >&2
        fi
    done
fi

# 3. Hand off to the user's command (default: bash shell with whatis on PATH).
exec "$@"
