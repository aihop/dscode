# dscode — mobile-first AI agent
# Usage:
#   docker build -t dscode .
#   docker run -it --rm -e DEEPSEEK_API_KEY=sk-... dscode chat
#
#   # Or mount host config:
#   docker run -it --rm \
#     -v ~/.config/dscode:/root/.config/dscode \
#     -e DEEPSEEK_API_KEY \
#     dscode chat
#
#   # Or set key once:
#   docker run -it --rm dscode auth login
#   docker commit $(docker ps -lq) dscode:configured

FROM rust:1.88-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y git && rm -rf /var/lib/apt/lists/*
COPY . .
RUN git submodule update --init --recursive
RUN cargo build --release -p dscode && \
    strip target/release/dscode

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/dscode /usr/local/bin/dscode
COPY --from=builder /build/www /usr/share/dscode/www
ENTRYPOINT ["dscode"]
CMD ["chat"]
