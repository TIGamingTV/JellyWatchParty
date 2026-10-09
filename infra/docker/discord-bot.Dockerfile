# JellyWatchParty Discord bot (sidecar of the session server).
# Build context: src/integrations/discord-bot

FROM rust:1.88-alpine AS builder

# Build mode: "dev" (fast compile, debug) or "release" (optimized)
ARG BUILD_MODE=release

RUN apk add --no-cache musl-dev mold
ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"

WORKDIR /usr/src/app

# Dependencies first (cached layer)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN if [ "$BUILD_MODE" = "release" ]; then \
        cargo build --release --locked; \
    else \
        cargo build --locked; \
    fi && rm -rf src

COPY src ./src
RUN touch src/main.rs && \
    if [ "$BUILD_MODE" = "release" ]; then \
        cargo build --release --locked && \
        cp target/release/jwp-discord-bot /usr/local/bin/; \
    else \
        cargo build --locked && \
        cp target/debug/jwp-discord-bot /usr/local/bin/; \
    fi

FROM alpine:3.21

# ca-certificates: TLS to Discord. No ports: the bot only makes outgoing
# connections (Discord gateway/REST, the session server's integration API).
RUN apk add --no-cache ca-certificates && \
    adduser -D -u 1000 appuser

COPY --from=builder /usr/local/bin/jwp-discord-bot /usr/local/bin/jwp-discord-bot

USER appuser
STOPSIGNAL SIGTERM
CMD ["jwp-discord-bot"]
