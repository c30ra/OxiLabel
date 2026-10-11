use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    let database_url = std::env::var("RTAG_DATABASE").unwrap_or_else(|_| "sqlite://data/rtag.db?mode=rwc".into());
    let addr = std::env::var("RTAG_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let web_root = std::env::var("RTAG_WEB_ROOT").unwrap_or_else(|_| "crates/rtag-ui/dist".into());

    let ui = ServeDir::new(&web_root).fallback(ServeFile::new(format!("{web_root}/index.html")));
    let app = rtag_server::app(&database_url).await.expect("failed to open database").fallback_service(ui);

    let listener = tokio::net::TcpListener::bind(&addr).await.expect("failed to bind");
    println!("rtag listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
