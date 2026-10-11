use std::path::PathBuf;

use axum::http::{HeaderValue, header};
use tower::Layer;
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

#[tokio::main]
async fn main() {
    let config = rtag_server::Config {
        database_url: env_or("RTAG_DATABASE", "sqlite://data/rtag.db?mode=rwc"),
        images_root: PathBuf::from(env_or("RTAG_IMAGES_ROOT", "images")),
        cache_dir: PathBuf::from(env_or("RTAG_CACHE_DIR", "data/cache")),
    };
    let addr = env_or("RTAG_ADDR", "0.0.0.0:3000");
    let web_root = env_or("RTAG_WEB_ROOT", "crates/rtag-ui/dist");

    let ui = ServeDir::new(&web_root).fallback(ServeFile::new(format!("{web_root}/index.html")));
    // The UI files keep their names across builds, so browsers must revalidate them (a cheap 304)
    // instead of running a stale build.
    let ui = SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")).layer(ui);
    let app = rtag_server::app(config).await.expect("failed to open database").fallback_service(ui);

    let listener = tokio::net::TcpListener::bind(&addr).await.expect("failed to bind");
    println!("rtag listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
