# Clawback in a web browser: the Linux desktop build, streamed by Selkies.
# Build from the repository root:
#   docker build -f packaging/docker/Dockerfile -t clawback-web .
# See docs/docker.md for running it.
#
# By default this compiles Clawback. Releases instead pass
# --build-arg BINARY=prebuilt with the release builds' executables at
# target/docker/<amd64|arm64>/clawback, so no stage compiles under emulation.
ARG BINARY=source

FROM rust:1-trixie AS source
# Unset, a source build never checks for updates (see src/updates.rs).
ARG CLAWBACK_UPDATE_CHANNEL
ARG CLAWBACK_BUILD_COMMIT
WORKDIR /src
# rust-toolchain.toml pins the compiler; install it in its own layer.
COPY rust-toolchain.toml .
RUN rustup toolchain install
COPY . .
RUN cargo build --release --locked && cp target/release/clawback /clawback

FROM scratch AS prebuilt
ARG TARGETARCH
COPY target/docker/${TARGETARCH}/clawback /clawback

FROM ${BINARY} AS binary

FROM ghcr.io/linuxserver/baseimage-selkies:debiantrixie

# There is no GPU in most servers: Mesa's software Vulkan (lavapipe) and GL
# (llvmpipe) draw the map. winit loads the X11/Wayland libraries at runtime.
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        libegl1 \
        libgl1-mesa-dri \
        libvulkan1 \
        libwayland-client0 \
        libx11-xcb1 \
        libxcursor1 \
        libxi6 \
        libxkbcommon-x11-0 \
        libxkbcommon0 \
        libxrandr2 \
        mesa-vulkan-drivers && \
    apt-get autoclean && \
    rm -rf /var/lib/apt/lists/* /var/tmp/* /tmp/* && \
    mkdir -p /data

COPY --from=binary /clawback /usr/bin/clawback
COPY packaging/docker/root/ /
RUN chmod 755 /defaults/autostart /defaults/autostart_wayland

# Clawback opens CLAWBACK_DATA at startup; map each folder to scan beneath it.
# The HARDEN_* settings make this a single-app kiosk: no terminal, file
# transfer, sudo or window close. Override them with -e to relax that.
ENV TITLE=Clawback \
    PIXELFLUX_WAYLAND=true \
    NO_DECOR=true \
    HARDEN_DESKTOP=true \
    HARDEN_OPENBOX=true \
    CLAWBACK_CONTAINER=true \
    CLAWBACK_DATA=/data \
    CLAWBACK_ALLOW_DELETE=true

EXPOSE 3001
VOLUME /config
