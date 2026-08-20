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
ARG TARGETPLATFORM
RUN echo ${TARGETPLATFORM}
RUN apt-get update && \
    apt-get install -y --no-install-recommends curl tar ca-certificates xz-utils && \
    rm -rf /var/lib/apt/lists/*

RUN if [ "${TARGETPLATFORM}" = "linux/amd64" ]; then \
    curl -fsSL https://github.com/Dusk-Labs/ffmpeg-static/releases/download/ffmpeg-all-0.0.1/ffmpeg -o ffmpeg && \
    echo "65d6b12fe32918e041a4513059c340e47b1a2e80c0d92c481c3889a31a470e9a  ffmpeg" | sha256sum -c - && \
    curl -fsSL https://github.com/Dusk-Labs/ffmpeg-static/releases/download/ffmpeg-all-0.0.1/ffprobe -o ffprobe && \
    echo "76e253d0cad674300280e556d8e61d86e6b301dd526cfd68191449221a004690  ffprobe" | sha256sum -c - \
    ; fi
    
RUN if [ "${TARGETPLATFORM}" = "linux/arm64" ]; then \
    curl -fsSL https://johnvansickle.com/ffmpeg/old-releases/ffmpeg-5.1.1-arm64-static.tar.xz -o ffmpeg.tar.xz && \
    echo "49f9beb7690afcbd4832d3577d9f0c87374d63c39cde5097dfd52d61b24b4855  ffmpeg.tar.xz" | sha256sum -c - && \
    tar --strip-components 1 -xf ffmpeg.tar.xz \
    ; fi
    
RUN if [ "${TARGETPLATFORM}" = "linux/arm/v7" ]; then \
    curl -fsSL https://johnvansickle.com/ffmpeg/old-releases/ffmpeg-5.1.1-armhf-static.tar.xz -o ffmpeg.tar.xz && \
    echo "7c9e39c20c3ccd2571013bac969e1239d29c226f0650fa220404d633c9c47d7e  ffmpeg.tar.xz" | sha256sum -c - && \
    tar --strip-components 1 -xf ffmpeg.tar.xz \
    ; fi
    
RUN chmod +x /static/ffmpeg && chmod +x /static/ffprobe


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
