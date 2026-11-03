use std::{net::SocketAddr, path::PathBuf, sync::Arc};

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
    let config = oxilabel_server::Config {
        database_url: env_or("OXILABEL_DATABASE", "sqlite://data/oxilabel.db?mode=rwc"),
        images_root: PathBuf::from(env_or("OXILABEL_IMAGES_ROOT", "images")),
        cache_dir: PathBuf::from(env_or("OXILABEL_CACHE_DIR", "data/cache")),
        export_dir: PathBuf::from(env_or("OXILABEL_EXPORT_DIR", "data/exports")),
        upload_dir: PathBuf::from(env_or("OXILABEL_UPLOAD_DIR", "data/uploads")),
        host_dialog: desktop_dialog(),
    };
    let addr = env_or("OXILABEL_ADDR", "0.0.0.0:3000");
    let web_root = env_or("OXILABEL_WEB_ROOT", "crates/oxilabel-ui/dist");

    let ui = ServeDir::new(&web_root).fallback(ServeFile::new(format!("{web_root}/index.html")));
    // The UI files keep their names across builds, so browsers must revalidate them (a cheap 304)
    // instead of running a stale build.
    let ui = SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")).layer(ui);
    let app = oxilabel_server::app(config).await.expect("failed to open database").fallback_service(ui);

    let listener = tokio::net::TcpListener::bind(&addr).await.expect("failed to bind");
    println!("OxiLabel listening on http://{addr}");
    // The peer address tells requests from this machine from others (ADR 0003).
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
    // The worker goes with the server, whatever it started below itself.
    oxilabel_server::stop_workers();
}

/// Ctrl+C, or SIGTERM (as Podman and systemd stop a process).
async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

/// The desktop's own file dialog (the XDG desktop portal's file chooser) for a folder or images, if
/// this server runs in a desktop session; headless (e.g. in Podman) there is none.
/// `OXILABEL_FOLDER_DIALOG=off` turns it off; `OXILABEL_FOLDER_DIALOG=answers:<file>` is for
/// automated tests: each time, the dialog "picks" the paths in that file, one per line (a folder
/// pick takes the first), is cancelled if the file is empty, or fails if it says `unavailable`.
fn desktop_dialog() -> Option<oxilabel_server::HostDialog> {
    use oxilabel_server::HostPick;
    match std::env::var("OXILABEL_FOLDER_DIALOG").unwrap_or_default().as_str() {
        "off" => return None,
        setting if setting.starts_with("answers:") => {
            let file = PathBuf::from(&setting["answers:".len()..]);
            return Some(Arc::new(move |_: HostPick| {
                let answer = std::fs::read_to_string(&file).unwrap_or_default();
                Box::pin(async move {
                    match answer.trim() {
                        "" => Ok(vec![]),
                        "unavailable" => Err("no desktop dialog in this test".to_owned()),
                        paths => Ok(paths.lines().map(str::trim).filter(|line| !line.is_empty()).map(PathBuf::from).collect()),
                    }
                })
            }));
        }
        _ => {}
    }
    let desktop = ["WAYLAND_DISPLAY", "DISPLAY"].iter().any(|name| std::env::var_os(name).is_some());
    let dialog: oxilabel_server::HostDialog = Arc::new(|pick: HostPick| {
        Box::pin(async move {
            use ashpd::desktop::{
                ResponseError,
                file_chooser::{FileFilter, SelectedFiles},
            };
            let request = SelectedFiles::open_file().modal(true);
            let request = match pick {
                HostPick::Folder => request.title("Choose a folder of images").directory(true),
                HostPick::Images => {
                    let images = ["image/bmp", "image/jpeg", "image/png", "image/tiff", "image/webp"]
                        .into_iter()
                        .fold(FileFilter::new("Images"), FileFilter::mimetype);
                    request.title("Choose images").multiple(true).filter(images)
                }
            };
            let request = request.send().await.map_err(|err| err.to_string())?;
            match request.response() {
                Ok(files) => Ok(files.uris().iter().filter_map(|uri| uri.to_file_path().ok()).collect()),
                Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(vec![]),
                Err(err) => Err(err.to_string()),
            }
        })
    });
    desktop.then_some(dialog)
}
