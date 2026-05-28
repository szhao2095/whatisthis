# Build with `./build.sh` from this directory (it stages the whatis binary
# from ../target/release/, then runs `docker build -t whatis .`). Manual
# build is also fine — see the COPY whatis line below.
#
# debian-slim because the whatis binary is dynamically linked against glibc;
# alpine/musl would require rebuilding with a musl target.

FROM debian:bookworm-slim

# p7zip-full because the sample archives use WinZip AES encryption
# (compression method 99) which Debian's stock Info-ZIP `unzip` does not
# support. 7z also auto-detects the archive format regardless of the
# inner file's extension (samples often rename inner archives to .zip_x
# etc. to avoid AV scanning).
RUN apt-get update \
    && apt-get install -y --no-install-recommends p7zip-full ca-certificates vim-tiny \
    && rm -rf /var/lib/apt/lists/*

# whatis binary. Pre-stage it into this directory before building:
#   ./build.sh                                  # convenience wrapper
# or manually:
#   cp ../target/release/whatis ./whatis
#   docker build -t whatis .
COPY whatis /usr/local/bin/whatis
RUN chmod +x /usr/local/bin/whatis

# Encrypted sample archives. Each .zip in docker/samples/ is double-zipped
# with password 'infected' (a common convention for sharing potentially
# unsafe content like malware corpora).
COPY samples/ /samples/

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# /tmp will be remounted as tmpfs by the entrypoint when possible.
# To guarantee a real ramdisk regardless of container capabilities, run with:
#   docker run --tmpfs /tmp:rw,size=2G ...
# To allow the entrypoint to mount tmpfs itself, run with --cap-add SYS_ADMIN.
ENTRYPOINT ["/entrypoint.sh"]
CMD ["/bin/bash"]
