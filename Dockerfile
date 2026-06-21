# Backend image for the `sigil-search` binary (multi-stage Rust build).
FROM rust:1-bookworm AS build
WORKDIR /app
# C toolchain for native deps (zstd-sys via Tantivy, etc.).
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential pkg-config cmake \
 && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p sigil-cli

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 10001 sigil \
 && mkdir -p /data && chown sigil:sigil /data
COPY --from=build /app/target/release/sigil-search /usr/local/bin/sigil-search
USER sigil
# query API, ES `_bulk`, syslog (UDP), OTLP/HTTP
EXPOSE 9595 9200 4317 5514/udp
ENTRYPOINT ["sigil-search"]
CMD ["run", "/etc/sigil/sigil.yaml"]
