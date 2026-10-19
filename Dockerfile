# Build from parent directory: docker build -f bootleg-smuggler/Dockerfile -t smuggler-demo .
# Only source files are copied, never .env, git credentials or project tokens.
FROM rust:1.94-bookworm AS source
WORKDIR /build
COPY bootleg-core/Cargo.toml bootleg-core/Cargo.toml
COPY bootleg-core/src bootleg-core/src
COPY bootleg-core/fixtures bootleg-core/fixtures
COPY bootleg-smuggler/Cargo.toml bootleg-smuggler/Cargo.lock bootleg-smuggler/
COPY bootleg-smuggler/src bootleg-smuggler/src
COPY bootleg-smuggler/tests bootleg-smuggler/tests
WORKDIR /build/bootleg-smuggler

FROM source AS test
RUN rustup component add clippy && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings

FROM source AS build
RUN cargo build --locked --release --bin smuggler

FROM debian:bookworm-slim AS demo
COPY --from=build /build/bootleg-smuggler/target/release/smuggler /usr/local/bin/smuggler
ARG BOOTLEG_BUILD_COMMIT
ARG BOOTLEG_BUILD_DIRTY
ENV BOOTLEG_BUILD_COMMIT=${BOOTLEG_BUILD_COMMIT}
ENV BOOTLEG_BUILD_DIRTY=${BOOTLEG_BUILD_DIRTY:-true}
ENTRYPOINT ["/usr/local/bin/smuggler", "--"]
CMD ["/bin/sh", "-c", "trap 'exit 0' TERM INT; while :; do sleep 1; done"]
