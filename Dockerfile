FROM rust:1.83.0-alpine AS builder

COPY ./ ./app

WORKDIR /app

RUN apk update && \
    apk add openssh git musl-dev openssl-dev openssl-libs-static
RUN --mount=type=ssh rm -rf ~/.ssh/known_hosts && \
    umask 077; mkdir -p ~/.ssh && \
    ssh-keyscan github.com >> ~/.ssh/known_hosts

ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

RUN --mount=type=ssh cargo build

FROM alpine

COPY --from=builder /app/target/debug/hoku-loader /app/hoku-loader

ENTRYPOINT ["/app/hoku-loader"]
