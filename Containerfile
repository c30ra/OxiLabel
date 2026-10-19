FROM docker.io/library/rust:1 AS build
RUN rustup target add wasm32-unknown-unknown
WORKDIR /src
# wasm-bindgen-cli must match the wasm-bindgen crate version in Cargo.lock exactly.
COPY Cargo.lock .
RUN version=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p') \
    && cargo install wasm-bindgen-cli --version "$version" --locked
COPY . .
RUN scripts/build-ui.sh release
RUN cargo build --release -p oxilabel-server

FROM docker.io/library/debian:stable-slim
COPY --from=build /src/target/release/oxilabel-server /usr/local/bin/oxilabel-server
COPY --from=build /src/crates/oxilabel-ui/dist /srv/oxilabel/web
ENV OXILABEL_WEB_ROOT=/srv/oxilabel/web \
    OXILABEL_DATABASE="sqlite:///data/oxilabel.db?mode=rwc" \
    OXILABEL_CACHE_DIR=/data/cache \
    OXILABEL_EXPORT_DIR=/data/exports \
    OXILABEL_IMAGES_ROOT=/images
# /data holds the database and caches; mount your datasets read-only at /images.
VOLUME /data /images
EXPOSE 3000
CMD ["oxilabel-server"]
