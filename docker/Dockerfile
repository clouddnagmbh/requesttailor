# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The RequestTailor Authors
#
# `rt`, the RequestTailor command line, as a container for pipelines:
#
#   docker build -f docker/Dockerfile -t rt .
#   docker run --rm -v "$PWD:/work" rt run . --reporter junit=junit.xml
#
# Build: the pinned toolchain from rust-toolchain.toml (rustup installs it if the base image's
# Rust differs), `--locked`, release. Runtime: distroless (glibc, CA certificates, no shell,
# no package manager) as the non-root user `nonroot` (uid 65532) in /work. Nothing copyleft
# is linked into `rt`; the base images' own licenses (distroless: Apache-2.0, Debian
# packages) cover the rest of the image.

# Only the carrier of rustup and a C compiler (for ring); the toolchain itself comes from
# rust-toolchain.toml. Keep the version equal to it to save the download.
FROM docker.io/library/rust:1.96.0-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    rustup toolchain install \
    && rustup show active-toolchain \
    && cargo build -p rt-cli --release --locked \
    && install -D -m 0755 target/release/rt /out/rt \
    && /out/rt --version

FROM gcr.io/distroless/cc-debian12:nonroot
ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.title="rt" \
      org.opencontainers.image.description="Command line of RequestTailor: send requests and run OpenCollection collections in pipelines" \
      org.opencontainers.image.source="https://github.com/clouddnagmbh/requesttailor" \
      org.opencontainers.image.url="https://github.com/clouddnagmbh/requesttailor" \
      org.opencontainers.image.documentation="https://github.com/clouddnagmbh/requesttailor/blob/main/crates/rt-cli/README.md" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"
COPY --from=build /out/rt /usr/local/bin/rt
COPY LICENSE NOTICE /usr/share/doc/rt/
USER nonroot
WORKDIR /work
ENTRYPOINT ["rt"]
CMD ["--help"]
