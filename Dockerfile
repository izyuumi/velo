FROM rust:1.85-slim AS builder

RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config libssl-dev clang && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./ 
COPY src ./src
COPY templates ./templates

RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg ca-certificates && \
    rm -rf /var/lib/apt/lists/*

ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=8000
ENV DATA_DIR=/data

WORKDIR /app
COPY --from=builder /app/target/release/video_hosted /usr/local/bin/hls-tube
COPY templates ./templates

VOLUME ["/data"]
EXPOSE 8000

CMD ["/usr/local/bin/hls-tube"]
