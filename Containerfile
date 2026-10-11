FROM docker.io/library/rust:1 AS build
RUN rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli --version 0.2.129 --locked
WORKDIR /src
COPY . .
RUN scripts/build-ui.sh release
RUN cargo build --release -p rtag-server

FROM docker.io/library/debian:stable-slim
COPY --from=build /src/target/release/rtag-server /usr/local/bin/rtag-server
COPY --from=build /src/crates/rtag-ui/dist /srv/rtag/web
ENV RTAG_WEB_ROOT=/srv/rtag/web \
    RTAG_DATABASE="sqlite:///data/rtag.db?mode=rwc"
VOLUME /data
EXPOSE 3000
CMD ["rtag-server"]
