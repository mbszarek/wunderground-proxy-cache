FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release  

FROM gcr.io/distroless/cc:latest
COPY --from=builder /app/target/release/wunderground-cache /
EXPOSE 8080
CMD ["./wunderground-cache"]