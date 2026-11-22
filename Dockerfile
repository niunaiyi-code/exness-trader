FROM rust:latest as builder

WORKDIR /app
COPY . .

RUN cargo build --release -p gl-server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/gl-server /usr/local/bin/
COPY config.example.toml /app/config.toml

WORKDIR /app

EXPOSE 8080

CMD ["gl-server"]
