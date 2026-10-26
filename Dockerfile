FROM rust:1-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY .sqlx ./.sqlx
COPY crates ./crates
COPY migrations ./migrations
ENV SQLX_OFFLINE=true
RUN cargo build --release -p talentos-gateway

FROM debian:bookworm-slim
# chromium: payslip HTML→PDF with real Thai shaping; fonts: Thai glyph coverage.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 curl chromium fonts-noto-core fonts-thai-tlwg \
    && rm -rf /var/lib/apt/lists/*
ENV CHROME_BIN=/usr/bin/chromium
ENV PAYSLIP_STORE_DIR=/var/lib/talentos/payslips
RUN mkdir -p /var/lib/talentos/payslips
COPY --from=builder /app/target/release/talentos-gateway /usr/local/bin/talentos-gateway
EXPOSE 8080
ENTRYPOINT ["talentos-gateway"]