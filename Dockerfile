# syntax=docker/dockerfile:1
FROM rust:1.99-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07 AS rust-source
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
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
    cargo clean --release -p wot-server -p wot-server-runtime -p wot-resources \
        -p wot-accounts -p wot-battles -p wot-platform -p wot-player-administration \
        -p wot-player-sessions -p wot-shared-kernel -p wot-social && \
    cargo build --locked --release --bin wot-server && \
    cp target/release/wot-server /build/wot-server

FROM rust-source AS e2e-build
COPY tests/support/ tests/support/
RUN apt-get update && apt-get install -y --no-install-recommends jq \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
RUN --mount=type=cache,id=wot-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=wot-e2e-target,target=/build/target,sharing=locked \
    cargo clean -p wot-server -p wot-server-runtime -p wot-resources \
        -p wot-accounts -p wot-battles -p wot-platform -p wot-player-administration \
        -p wot-player-sessions -p wot-shared-kernel -p wot-social && \
    cargo test --locked -p wot-server --features integration-test-support --test garage \
        --no-run --message-format=json-render-diagnostics > /tmp/e2e-build.json && \
    test_executable=$(jq -er 'select(.reason == "compiler-artifact" and .target.name == "garage" and .profile.test) | .executable | strings' /tmp/e2e-build.json) && \
    cp "$test_executable" /build/wot-e2e && \
    cp target/debug/wot-server /build/wot-server

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS e2e
WORKDIR /opt/wot-server
COPY --from=e2e-build /build/wot-e2e /usr/local/bin/wot-e2e
# The web test launches the server using Cargo's compile-time executable path.
COPY --from=e2e-build /build/wot-server /build/target/debug/wot-server
ENTRYPOINT ["/usr/local/bin/wot-e2e"]

FROM node:22-bookworm-slim@sha256:e9f565d32859f0cbfb52125eafe8be39390f24dac302dc3cdf566e8ba3ab21ee AS web-build
WORKDIR /build
COPY package.json package-lock.json ./
COPY src/server-runtime/src/infrastructure/web/package.json src/server-runtime/src/infrastructure/web/package.json
RUN npm ci
COPY src/ src/
ARG WOT_SERVER_VERSION
RUN npm run build --workspace src/server-runtime/src/infrastructure/web

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
WORKDIR /opt/wot-server
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && chown 10001:10001 .
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/licenses/wot-server/
COPY --from=server-build /build/wot-server ./
COPY --from=web-build /build/src/server-runtime/src/infrastructure/web/dist/ ./web/
RUN ln -s web dashboard
COPY resources/ ./resources/
USER 10001:10001
EXPOSE 20016/udp 20017/udp 20017/tcp 8080/tcp
STOPSIGNAL SIGINT
ENTRYPOINT ["/opt/wot-server/wot-server"]
