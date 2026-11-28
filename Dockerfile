# syntax=docker/dockerfile:1
FROM rust:1.99-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07 AS rust-source
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY tools/ tools/
COPY src/ src/
COPY tests/*.rs tests/

FROM node:22-bookworm-slim@sha256:e9f565d32859f0cbfb52125eafe8be39390f24dac302dc3cdf566e8ba3ab21ee AS build-node

FROM rust-source AS rust-web-source
COPY --from=build-node /usr/local/bin/node /usr/local/bin/node
COPY --from=build-node /usr/local/lib/node_modules/npm/ /usr/local/lib/node_modules/npm/
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm
COPY package.json package-lock.json ./

FROM rust-web-source AS server-build
ARG WOT_SERVER_VERSION
ARG WOT_GAME_VERSION
RUN --mount=type=cache,id=wot-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=wot-release-target,target=/build/target,sharing=locked \
    --mount=type=cache,id=wot-npm-cache,target=/root/.npm,sharing=locked \
    cargo clean --release -p wot-server -p wot-server-runtime -p wot-web-build -p wot-resources \
        -p wot-accounts -p wot-battles -p wot-platform -p wot-player-administration \
        -p wot-player-sessions -p wot-shared-kernel -p wot-social \
        -p wot-store -p wot-training-rooms && \
    cargo build --locked --release --bin wot-server && \
    cp target/release/wot-server /build/wot-server

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
WORKDIR /opt/wot-server
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && chown 10001:10001 .
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/licenses/wot-server/
COPY --from=server-build /build/wot-server ./
COPY --from=server-build /build/src/server-runtime/src/infrastructure/web/dist/ ./web/
RUN ln -s web dashboard
COPY resources/ ./resources/
USER 10001:10001
EXPOSE 20016/udp 20017/udp 20017/tcp 8080/tcp
STOPSIGNAL SIGINT
ENTRYPOINT ["/opt/wot-server/wot-server"]
