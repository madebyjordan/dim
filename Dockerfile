FROM node:24.19.0-bookworm AS web
WORKDIR /ui
COPY ui/package.json ui/yarn.lock ui/.yarnrc.yml ./
RUN corepack enable && yarn install --immutable --mode=skip-build
COPY ui ./
RUN yarn run build

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


FROM rust:bullseye AS dim
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y \
    libva-dev \
    libva-drm2 \
    libva2 \
    sqlite3
WORKDIR /dim
COPY . ./
COPY --from=web /ui/build ui/build
ARG DATABASE_URL="sqlite://dim_dev.db"

# Sometimes we may need to quickly build a test image
ARG RUST_BUILD=release
RUN if [ "$RUST_BUILD" = "debug" ]; then \
        cargo build --features vaapi && \
        mv ./target/debug/dim ./target/dim \
    ; fi

RUN if [ "$RUST_BUILD" = "release" ]; then \
        cargo build --features vaapi --release && \
        mv ./target/release/dim ./target/dim \
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
COPY --from=ffmpeg /static/ffmpeg /opt/dim/utils/ffmpeg
COPY --from=ffmpeg /static/ffprobe /opt/dim/utils/ffprobe
COPY --from=dim /dim/target/dim /opt/dim/dim

RUN useradd --system --uid 10001 --home-dir /opt/dim --shell /usr/sbin/nologin dim && \
    mkdir -p /opt/dim/config /opt/dim/metadata /opt/dim/streaming_cache /opt/dim/logs && \
    chown -R dim:dim /opt/dim

EXPOSE 8000
VOLUME ["/opt/dim/config", "/opt/dim/metadata", "/opt/dim/streaming_cache", "/opt/dim/logs"]

ENV RUST_LOG=info
WORKDIR /opt/dim
USER dim
HEALTHCHECK --interval=30s --timeout=3s --start-period=20s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:8000/health/ready >/dev/null || exit 1
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["./dim"]
