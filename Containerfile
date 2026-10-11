FROM docker.io/library/rust:1 AS build
RUN rustup target add wasm32-unknown-unknown
WORKDIR /src
# wasm-bindgen-cli must match the wasm-bindgen crate version in Cargo.lock exactly.
COPY Cargo.lock .
RUN version=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p') \
    && cargo install wasm-bindgen-cli --version "$version" --locked
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
