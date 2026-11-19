# syntax=docker/dockerfile:1
FROM rust:1.99-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07 AS rust-source
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY tools/ tools/
COPY src/ src/
COPY tests/*.rs tests/

# A reused source layer can have older timestamps than the shared Cargo cache.
FROM rust-source AS extractor-build
RUN --mount=type=cache,id=wot-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=wot-release-target,target=/build/target,sharing=locked \
    cargo clean --release -p wot-extract -p wot-resources && \
    cargo build --locked --release -p wot-extract && \
    cp target/release/wot-extract /build/wot-extract

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS extractor
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/licenses/wot-server/
COPY --from=extractor-build /build/wot-extract /wot-extract
ENTRYPOINT ["/wot-extract"]

FROM rust-source AS server-build
ARG WOT_SERVER_VERSION
ARG WOT_GAME_VERSION
RUN --mount=type=cache,id=wot-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=wot-release-target,target=/build/target,sharing=locked \
    cargo clean --release -p wot-server -p wot-resources && \
    cargo build --locked --release --bin wot-server && \
    cp target/release/wot-server /build/wot-server

FROM node:22-bookworm-slim@sha256:e9f565d32859f0cbfb52125eafe8be39390f24dac302dc3cdf566e8ba3ab21ee AS web-build
WORKDIR /build
COPY package.json package-lock.json ./
COPY tools/web/package.json tools/web/package.json
RUN npm ci
COPY tools/web/ tools/web/
COPY src/ src/
ARG WOT_SERVER_VERSION
RUN npm run build --workspace tools/web

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
WORKDIR /opt/wot-server
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && chown 10001:10001 .
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/licenses/wot-server/
COPY --from=server-build /build/wot-server ./
COPY --from=web-build /build/tools/web/dist/ ./web/
RUN ln -s web dashboard
COPY resources/ ./resources/
USER 10001:10001
EXPOSE 20016/udp 20017/udp 20017/tcp 8080/tcp
STOPSIGNAL SIGINT
ENTRYPOINT ["/opt/wot-server/wot-server"]
