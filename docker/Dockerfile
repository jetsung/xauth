FROM rust:1.95-trixie AS builder

WORKDIR /app
COPY . .

RUN cargo build --release

FROM gcr.io/distroless/cc-debian13:nonroot

WORKDIR /app

COPY --from=builder /app/target/release/xauth /usr/local/bin/xauth

EXPOSE 3000

ENTRYPOINT ["xauth"]