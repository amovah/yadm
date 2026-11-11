# Build environment for YADM on Linux. The -dev packages (headers and
# pkg-config files) live only in this image; the host needs just the runtime
# libraries, which any desktop already has. Debian's glibc is older than most
# desktops', so the binaries run on newer distros too.
ARG RUST_VERSION=1.99
FROM rust:${RUST_VERSION}-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libfontconfig-dev libfreetype-dev \
        libxkbcommon-dev libxkbcommon-x11-dev \
        libwayland-dev libx11-dev libx11-xcb-dev libxcb1-dev \
        libssl-dev zlib1g-dev xz-utils \
    && rm -rf /var/lib/apt/lists/*

# aria2c for bundling, built like scripts/build-aria2.sh does for macOS: only
# OpenSSL and zlib (plus libc/libstdc++), so it runs on any current distro.
# Debian's own aria2c drags in libaria2, libxml2, c-ares, libssh2, ...
ARG ARIA2_VERSION=1.37.0
RUN curl -fsSL "https://github.com/aria2/aria2/releases/download/release-${ARIA2_VERSION}/aria2-${ARIA2_VERSION}.tar.xz" \
        | tar xJ -C /tmp \
    && cd /tmp/aria2-${ARIA2_VERSION} \
    && ./configure --disable-dependency-tracking \
        --with-openssl --without-gnutls --without-libgcrypt --without-libnettle \
        --without-libxml2 --without-libexpat --without-libcares --without-libssh2 \
        --without-sqlite3 --without-libuv --enable-bittorrent \
        CXXFLAGS="-O2" > /dev/null \
    && make -j"$(nproc)" > /dev/null \
    && install -s src/aria2c /opt/aria2c \
    && cd / && rm -rf /tmp/aria2-${ARIA2_VERSION}
