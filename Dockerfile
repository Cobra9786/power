FROM rust:alpine as builder

WORKDIR /opt/runes-dex

ADD . .

RUN apk add --no-cache gcc musl-dev openssl libressl-dev pkgconfig  && cargo build --release && cp ./target/release/runes-dex ./runes-dex

FROM alpine:latest

WORKDIR /opt/runes-dex

ENV RUST_LOG info

COPY --from=builder /opt/runes-dex/runes-dex /opt/runes-dex/
COPY --from=builder /opt/runes-dex/config.toml /opt/runes-dex/
RUN mkdir ./db/

EXPOSE 3000

CMD ["./ruined", "-c config.toml"]
