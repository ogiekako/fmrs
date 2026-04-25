FROM rust:bookworm AS builder

WORKDIR /app
COPY rust ./rust
RUN cargo build --manifest-path rust/Cargo.toml --release

FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/rust/target/release/fmrs /usr/local/bin/fmrs

ENV HOST=0.0.0.0
ENV PORT=8080

EXPOSE 8080

CMD ["fmrs", "server"]
