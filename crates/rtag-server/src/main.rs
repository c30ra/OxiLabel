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

/// Where the worker listens when the server starts it and no URL is given.
const DEFAULT_WORKER_URL: &str = "http://127.0.0.1:3112";

#[tokio::main]
async fn main() {
    let worker_command = std::env::var("RTAG_WORKER_COMMAND").ok().filter(|command| !command.trim().is_empty());
    let mut worker_url = std::env::var("RTAG_WORKER_URL").ok().filter(|url| !url.is_empty());
    if let Some(command) = worker_command {
        let url = worker_url.get_or_insert_with(|| DEFAULT_WORKER_URL.to_owned()).clone();
        tokio::spawn(supervise_worker(command, url));
    }
    let config = rtag_server::Config {
        database_url: env_or("RTAG_DATABASE", "sqlite://data/rtag.db?mode=rwc"),
        images_root: PathBuf::from(env_or("RTAG_IMAGES_ROOT", "images")),
        cache_dir: PathBuf::from(env_or("RTAG_CACHE_DIR", "data/cache")),
        export_dir: PathBuf::from(env_or("RTAG_EXPORT_DIR", "data/exports")),
        upload_dir: PathBuf::from(env_or("RTAG_UPLOAD_DIR", "data/uploads")),
        folder_dialog: desktop_folder_dialog(),
        worker_url,
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
    // The peer address tells requests from this machine from others (ADR 0003).
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}

/// Runs the pre-annotation worker (`RTAG_WORKER_COMMAND`, through the shell) and starts it again
/// whenever it exits. It is told to listen where `url` points, in `RTAG_WORKER_ADDR`.
async fn supervise_worker(command: String, url: String) {
    let addr = url.trim_start_matches("http://").trim_end_matches('/').to_owned();
    loop {
        println!("starting the worker: {command}");
        let child = tokio::process::Command::new("sh").arg("-c").arg(&command).env("RTAG_WORKER_ADDR", &addr).kill_on_drop(true).spawn();
        match child {
            Ok(mut child) => match child.wait().await {
                Ok(status) => eprintln!("the worker stopped ({status}); starting it again in 5 s"),
                Err(err) => eprintln!("lost the worker: {err}"),
            },
            Err(err) => eprintln!("cannot start the worker: {err}; trying again in 5 s"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// The desktop's own folder dialog (the XDG desktop portal's file chooser), if this server runs in a
/// desktop session; headless (e.g. in Podman) there is none. `RTAG_FOLDER_DIALOG=off` turns it
/// off; `RTAG_FOLDER_DIALOG=answers:<file>` is for automated tests: each time, the dialog "picks"
/// the folder named in that file, is cancelled if the file is empty, or fails if it says
/// `unavailable`.
fn desktop_folder_dialog() -> Option<rtag_server::FolderDialog> {
    match std::env::var("RTAG_FOLDER_DIALOG").unwrap_or_default().as_str() {
        "off" => return None,
        setting if setting.starts_with("answers:") => {
            let file = PathBuf::from(&setting["answers:".len()..]);
            return Some(Arc::new(move || {
                let answer = std::fs::read_to_string(&file).unwrap_or_default();
                Box::pin(async move {
                    match answer.trim() {
                        "" => Ok(None),
                        "unavailable" => Err("no folder dialog in this test".to_owned()),
                        path => Ok(Some(PathBuf::from(path))),
                    }
                })
            }));
        }
        _ => {}
    }
    let desktop = ["WAYLAND_DISPLAY", "DISPLAY"].iter().any(|name| std::env::var_os(name).is_some());
    let dialog: rtag_server::FolderDialog = Arc::new(|| {
        Box::pin(async {
            use ashpd::desktop::{ResponseError, file_chooser::SelectedFiles};
            let request = SelectedFiles::open_file()
                .title("Choose a folder of images")
                .modal(true)
                .directory(true)
                .send()
                .await
                .map_err(|err| err.to_string())?;
            match request.response() {
                Ok(files) => Ok(files.uris().first().and_then(|uri| uri.to_file_path().ok())),
                Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(None),
                Err(err) => Err(err.to_string()),
            }
        })
    });
    desktop.then_some(dialog)
}
