# syntax=docker/dockerfile:1.7
#
# reasoning-engine: the Rust MCP server as a static binary on distroless.
#
#   docker build -t reasoning-engine .
#   docker run --rm -p 8765:8765 -v re-data:/data \
#     -e REASONING_ENGINE_HTTP_TOKEN="$(openssl rand -hex 32)" reasoning-engine
#
# The image's only job is serving MCP over HTTP, so it is built with the
# `local-host` feature (plus `trust`: signed run packs, key in /data) and
# starts `serve --http`. A public bind always needs
# the bearer token; the server refuses to start without it. Add `--ui` to
# the command to mount the read-only audit UI at /ui (same token).
# Base images are pinned by digest; bump them deliberately.

FROM rust:1.99.1-alpine3.22@sha256:2b3b5cdd9112f19366d21b94541ffcd3d58bf838f696451a0d50c8032088cb15 AS build
# musl is Alpine's libc, so the binary links statically.
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p re-cli --features local-host,trust \
 && cp target/release/reasoning-engine /reasoning-engine \
 && mkdir -p /data/runs

FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
COPY --from=build /reasoning-engine /usr/local/bin/reasoning-engine
COPY --from=build --chown=65532:65532 /data /data
ENV REASONING_ENGINE_DB=/data/reasoning.db \
    REASONING_ENGINE_RUNS_DIR=/data/runs \
    REASONING_ENGINE_CONFIG=/data/config.toml \
    REASONING_ENGINE_SIGNING_KEY=/data/signing.key
VOLUME ["/data"]
EXPOSE 8765
USER nonroot
ENTRYPOINT ["/usr/local/bin/reasoning-engine"]
CMD ["serve", "--http", "--host", "0.0.0.0", "--port", "8765", "--unsafe-bind-public", "--bearer-token-env", "REASONING_ENGINE_HTTP_TOKEN"]
