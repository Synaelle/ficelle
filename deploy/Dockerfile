# syntax=docker/dockerfile:1
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The server image: the server and its web pages (the admin page, setup) on
# distroless, no shell, not root. Build from the repository root:
#   docker build -f deploy/Dockerfile -t ficelle-server .
# CHANNEL=release stamps the workspace version, preview stamps
# preview-{commit}. COMMIT and COMMIT_DATE stand in for git, which the build
# does not see (Dockerfile.dockerignore, beside this file, keeps .git out).

# the names below, and the binary's path at the end, are written by
# tools/brand.mjs from brand.json
ARG BINARY=ficelle-server
ARG ENV_PREFIX=FICELLE
ARG SLUG=ficelle

FROM node:24-trixie-slim@sha256:54f84b4cf8432b21aaffd5d3ce8d41e12cea0d10d33c09d2f3c9879d49d6b446 AS node

FROM rust:1.99.0-trixie@sha256:81015672d6939b0265f09adf96d5de125482bb1330106076956b0518e74edc90 AS build
ARG CHANNEL=preview
ARG COMMIT=
ARG COMMIT_DATE=
ARG BINARY
ARG SLUG
COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
 && npm install -g --no-fund --no-audit pnpm@12.15.0
# the web pages carry the e2ee client as WebAssembly, built by tools/wasm.mjs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install --locked wasm-bindgen-cli --version 0.2.129
WORKDIR /src
# dependencies first, so a source change does not fetch them again
COPY pnpm-lock.yaml pnpm-workspace.yaml ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store pnpm fetch
COPY . .
# rust-toolchain.toml names the version and the wasm target
RUN rustup toolchain install
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --offline --frozen-lockfile --ignore-scripts
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,id=target-${CHANNEL} \
    set -eu; \
    case "$CHANNEL" in \
      release) export APP_RELEASE=1 ;; \
      preview) export APP_PREVIEW=1 ;; \
      *) echo "CHANNEL is release or preview" >&2; exit 1 ;; \
    esac; \
    export APP_COMMIT="$COMMIT" APP_COMMIT_DATE="$COMMIT_DATE"; \
    pnpm --filter @app/client build; \
    cargo build --release --locked -p server; \
    mkdir -p /out/bin /out/web /out/root; \
    install -d -m 700 /out/root/data; \
    cp "target/release/$BINARY" /out/bin/; \
    cp -r apps/client/dist/. /out/web/; \
    node tools/fonts-mirror.mjs /out/web/fonts

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e
ARG BINARY
ARG ENV_PREFIX
ARG SLUG
COPY --from=build /out/bin/ /usr/local/bin/
COPY --from=build /out/web/ /usr/share/${SLUG}/web/
# the volume takes this folder's owner and mode when it is first made. The
# folder is copied from its parent so its own mode comes along
COPY --from=build --chown=65532:65532 /out/root/ /
ENV ${ENV_PREFIX}_SERVER__BIND=0.0.0.0:8080 \
    ${ENV_PREFIX}_STORAGE__DATA_DIR=/data \
    ${ENV_PREFIX}_SERVER__WEB_DIR=/usr/share/${SLUG}/web \
    APP_BINARY=/usr/local/bin/${BINARY}
# the config file, when there is one, sits with the data
WORKDIR /data
VOLUME /data
EXPOSE 8080
USER 65532:65532
# no shell here: the server asks its own /health
HEALTHCHECK --interval=30s --timeout=10s --start-period=20s --retries=3 \
    CMD ["/usr/local/bin/ficelle-server", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/ficelle-server"]
