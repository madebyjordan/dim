FROM node:24.19.0-bookworm AS web
WORKDIR /eclipse
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY eclipse/package.json eclipse/package.json
RUN corepack pnpm --dir eclipse install --frozen-lockfile
COPY api-contract api-contract
COPY scripts/generate-api-contract.mjs scripts/generate-api-contract.mjs
COPY eclipse eclipse
RUN corepack pnpm --dir eclipse build

FROM debian:bookworm-slim AS ffmpeg
ARG DEBIAN_FRONTEND=noninteractive
WORKDIR /static
ARG TARGETARCH
RUN apt-get update && \
    apt-get install -y --no-install-recommends curl tar ca-certificates xz-utils coreutils findutils && \
    rm -rf /var/lib/apt/lists/*
COPY scripts/install-ffmpeg9-linux.sh /install-ffmpeg9-linux.sh
RUN bash /install-ffmpeg9-linux.sh /static "${TARGETARCH}"


FROM rust:bullseye AS eclipse
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y \
    libva-dev \
    libva-drm2 \
    libva2 \
    sqlite3
WORKDIR /eclipse
COPY . ./
COPY --from=web /eclipse/eclipse/build eclipse/build
ARG DATABASE_URL="sqlite://dim_dev.db"

# Sometimes we may need to quickly build a test image
ARG RUST_BUILD=release
RUN if [ "$RUST_BUILD" = "debug" ]; then \
        cargo build --features vaapi --locked && \
        mv ./target/debug/eclipse ./target/eclipse \
    ; fi

RUN if [ "$RUST_BUILD" = "release" ]; then \
        cargo build --features vaapi --release --locked && \
        mv ./target/release/eclipse ./target/eclipse \
    ; fi

FROM debian:bullseye
ENV RUST_BACKTRACE=full
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libfontconfig \
    libfribidi0 \
    libharfbuzz0b \
    libtheora0 \
    libva-drm2 \
    libva2 \
    libvorbis0a \
    libvorbisenc2 curl tini \
    && rm -rf /var/lib/apt/lists/*
COPY --from=ffmpeg /static/ffmpeg /opt/eclipse/utils/ffmpeg
COPY --from=ffmpeg /static/ffprobe /opt/eclipse/utils/ffprobe
COPY --from=eclipse /eclipse/target/eclipse /opt/eclipse/eclipse
COPY scripts/docker-entrypoint.sh /usr/local/bin/eclipse-entrypoint

RUN useradd --system --uid 10001 --home-dir /opt/eclipse --shell /usr/sbin/nologin eclipse && \
    mkdir -p /opt/eclipse/config /opt/eclipse/metadata /opt/eclipse/streaming_cache /opt/eclipse/logs \
             /opt/dim/config /opt/dim/metadata /opt/dim/streaming_cache /opt/dim/logs && \
    ln -s /opt/eclipse/utils /opt/dim/utils && \
    chmod +x /usr/local/bin/eclipse-entrypoint && \
    chown -R eclipse:eclipse /opt/eclipse /opt/dim

EXPOSE 8000
VOLUME ["/opt/eclipse/config", "/opt/eclipse/metadata", "/opt/eclipse/streaming_cache", "/opt/eclipse/logs"]

ENV RUST_LOG=info
WORKDIR /opt/eclipse
USER eclipse
HEALTHCHECK --interval=30s --timeout=3s --start-period=20s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:8000/health/ready >/dev/null || exit 1
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/eclipse-entrypoint"]
