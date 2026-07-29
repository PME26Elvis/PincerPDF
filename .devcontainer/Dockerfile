# syntax=docker/dockerfile:1.7
FROM node:22.16.0-bookworm-slim AS node

FROM rust:1.97.1-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=always \
    CARGO_BUILD_JOBS=4 \
    TRUNK_SKIP_VERSION_CHECK=true

COPY --from=node /usr/local/ /usr/local/

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       build-essential ca-certificates chromium clang cmake curl ffmpeg file git \
       imagemagick jq libayatana-appindicator3-dev libssl-dev librsvg2-dev \
       libwebkit2gtk-4.1-dev libxdo-dev lld mupdf-tools ninja-build patchelf \
       pkg-config python3 python3-venv qpdf wget xvfb \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add clippy rustfmt \
    && rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked --version 0.21.14

WORKDIR /workspace/PincerPDF
