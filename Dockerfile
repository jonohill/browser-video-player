FROM rust:1.75.0 AS builder

WORKDIR /usr/src/bvp
COPY . .

RUN cargo install --path .

FROM debian:11.6-slim

RUN apt-get update && apt-get install -y ffmpeg

COPY --from=builder /usr/local/cargo/bin/browser-video-player /usr/local/bin/browser-video-player

ENTRYPOINT [ "browser-video-player" ]
