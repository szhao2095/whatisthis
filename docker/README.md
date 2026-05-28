# whatis test container

Lightweight Debian-slim container that bundles the `whatis` binary, ships
encrypted sample archives from `docker/samples/`, and on startup mounts a
ramdisk at `/tmp` and extracts the (double-zipped, password `infected`)
samples into `/tmp/samples/<archive-name>/`.

## Build

```bash
cargo build --release --bin whatis     # ensure the binary exists
cd docker
./build.sh                             # stages the binary, runs docker build
```

`build.sh` copies `../target/release/whatis` into the `docker/` directory
(Docker can't `COPY` from outside the build context) and then runs
`docker build -t whatis .` from here. Override the tag with
`WHATIS_IMAGE=mytag ./build.sh`.

If you'd rather build manually:

```bash
cp ../target/release/whatis ./whatis
docker build -t whatis .
```

## Run

**The decrypted samples must live in memory** — if they land on Docker's
overlay filesystem the host's AV will quarantine them. The entrypoint
refuses to proceed unless it can put extraction on a tmpfs. Pick one:

```bash
# Recommended: have Docker mount /tmp as tmpfs.
docker run --rm -it --tmpfs /tmp:rw,size=2G whatis

# Let the entrypoint mount tmpfs itself (needs CAP_SYS_ADMIN).
docker run --rm -it --cap-add SYS_ADMIN whatis

# Use /dev/shm (which Docker mounts as tmpfs by default). The entrypoint
# falls back here automatically when neither of the above is supplied; bump
# the size with --shm-size since the default (64 MB) is small.
docker run --rm -it --shm-size=2g whatis
```

In `/dev/shm` mode the entrypoint symlinks `/tmp/samples → /dev/shm/samples`
so the same paths work either way.

Add `-e WHATIS_DEBUG=1` to make the entrypoint print 7z output during
extraction.

Inside the container:

```bash
ls /tmp/samples/                       # one subdir per outer .zip
whatis /tmp/samples/<dir>/             # classify the extracted files
whatis -c --tficf /tmp/samples/<dir>/<file>
```

## Adding samples

Drop more `.zip` archives into `docker/samples/`. Each must be:

- A `.zip` containing one inner `.zip`
- Both layers encrypted with password `infected`

(Single-layer zips are tolerated — the entrypoint moves contents to the
output dir as-is when no inner zip is found.)

Rebuild the image after adding samples.

## Tunables

| env var | default | meaning |
|---|---|---|
| `WHATIS_TMPFS_SIZE` | `2G` | size of the entrypoint-mounted tmpfs (mode 2 only) |
