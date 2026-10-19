//! The CLI against a real server (in-memory database, temporary folders) and a fake worker, each
//! on a free local port.

use std::{
    io::{Cursor, Read},
    net::SocketAddr,
    path::Path,
    sync::mpsc,
};

use oxilabel_cli::{CliError, exit_code, run};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Server {
    url: String,
    images: TempDir,
    _dirs: Vec<TempDir>,
}

/// Starts a server and a fake worker in a thread of their own; returns the server's URL.
fn server() -> Server {
    let images = TempDir::new().unwrap();
    let dirs: Vec<TempDir> = (0..4).map(|_| TempDir::new().unwrap()).collect();
    let config = oxilabel_server::Config {
        database_url: "sqlite::memory:".into(),
        images_root: images.path().to_owned(),
        cache_dir: dirs[0].path().to_owned(),
        export_dir: dirs[1].path().to_owned(),
        upload_dir: dirs[2].path().to_owned(),
        folder_dialog: None,
    };
    let (ready, address) = mpsc::channel();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            let worker = axum::Router::new()
                .route("/health", axum::routing::get(|| async { axum::Json(json!({"device": "fake"})) }))
                .route("/predict", axum::routing::post(|| async {
                    axum::Json(json!({"predictions": [
                        {"class": "crack", "confidence": 0.9, "shape": {"type": "rectangle", "min": {"x": 1.0, "y": 1.0}, "max": {"x": 5.0, "y": 5.0}}},
                        {"class": "crack", "confidence": 0.2, "shape": {"type": "rectangle", "min": {"x": 2.0, "y": 2.0}, "max": {"x": 6.0, "y": 6.0}}},
                    ]}))
                }));
            let worker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let worker_url = format!("http://{}", worker_listener.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(worker_listener, worker).await.unwrap() });
            let app = oxilabel_server::app(config).await.unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            ready.send((listener.local_addr().unwrap(), worker_url)).unwrap();
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
        });
    });
    let (address, worker_url) = address.recv().unwrap();
    let url = format!("http://{address}");
    let setup = reqwest::blocking::Client::new().post(format!("{url}/api/setup")).json(&json!({"username": "admin", "password": "admin password"})).send().unwrap();
    assert!(setup.status().is_success());
    let cookie = setup.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_owned();
    let api = Api { url: url.clone(), cookie };
    api.call("PUT", "/api/settings/worker", json!({"url": worker_url}));
    Server { url, images, _dirs: dirs }
}

/// Direct API calls for setting up what the CLI then works on.
struct Api {
    url: String,
    cookie: String,
}

impl Api {
    fn call(&self, method: &str, path: &str, body: Value) -> Value {
        let request = reqwest::blocking::Client::new().request(method.parse().unwrap(), format!("{}{path}", self.url)).header("cookie", &self.cookie).json(&body);
        let response = request.send().unwrap();
        assert!(response.status().is_success(), "{method} {path}: {}", response.status());
        response.json().unwrap_or(Value::Null)
    }
}

/// Runs `oxilabel <line>` with its session in `config`, the password (if asked) from `stdin`.
fn oxilabel(config: &Path, line: &str, stdin: &str) -> (Result<(), CliError>, String) {
    let mut args = vec!["oxilabel".to_owned(), "--config-dir".to_owned(), config.display().to_string()];
    args.extend(shell_words(line));
    let mut out = Vec::new();
    let result = run(args, &mut out, &mut Cursor::new(stdin.as_bytes()));
    (result, String::from_utf8(out).unwrap())
}

/// Splits on spaces, keeping "quoted parts" together.
fn shell_words(line: &str) -> Vec<String> {
    line.split('"').enumerate().flat_map(|(i, part)| if i % 2 == 1 { vec![part.to_owned()] } else { part.split_whitespace().map(str::to_owned).collect() }).collect()
}

fn ok(result: (Result<(), CliError>, String)) -> String {
    let (result, out) = result;
    assert!(result.is_ok(), "{}: {out}", result.unwrap_err());
    out
}

fn png(path: &Path, width: u32, height: u32, shade: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(width, height, image::Rgb([shade, 5, 5])).save(path).unwrap();
}

#[test]
fn a_session_imports_exports_and_pre_annotates_through_the_server() {
    let server = server();
    let config = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();
    let c = config.path();

    // Signing in: a wrong password fails with the server's reason; the right one is kept.
    let (result, _) = oxilabel(c, &format!("login --server {}/ --user admin --password-stdin", server.url), "nope\n");
    let err = result.unwrap_err();
    assert_eq!((exit_code(&Err(CliError(err.0.clone(), err.1))), err.0.as_str()), (1, "wrong username or password"));
    assert!(ok(oxilabel(c, &format!("login --server {} --user admin --password-stdin", server.url), "admin password\n")).contains("Signed in"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(c.join("session")).unwrap().permissions().mode() & 0o777, 0o600);
    }

    // A project made through the API, with two images and a label.
    let cookie = std::fs::read_to_string(c.join("session")).unwrap();
    let cookie: Value = serde_json::from_str(&cookie).unwrap();
    let api = Api { url: server.url.clone(), cookie: cookie["cookie"].as_str().unwrap().to_owned() };
    let project = api.call("POST", "/api/projects", json!({"name": "EL defects"}));
    let p = project["id"].as_i64().unwrap();
    api.call("POST", &format!("/api/projects/{p}/labels"), json!({"name": "Crack", "color": "#ff0000"}));
    png(&server.images.path().join("a.png"), 40, 20, 1);
    png(&server.images.path().join("b.png"), 40, 20, 2);
    api.call("POST", &format!("/api/projects/{p}/imports"), json!({"paths": ["a.png", "b.png"]}));
    assert!(ok(oxilabel(c, "projects", "")).contains("EL defects  (2 images, 0 annotated, 0 jobs)"));

    // CVAT: a dry run writes nothing, then the import maps "crack" to the project's "Crack".
    let xml = work.path().join("annotations.xml");
    std::fs::write(&xml, r#"<annotations><version>1.1</version><image id="0" name="a.png" width="40" height="20"><box label="crack" xtl="1" ytl="1" xbr="9" ybr="9" z_order="0"/><box label="dust" xtl="1" ytl="1" xbr="3" ybr="3" z_order="0"/></image></annotations>"#).unwrap();
    let line = format!("import cvat --project \"EL defects\" {} --label crack=Crack --label dust=skip", xml.display());
    assert!(ok(oxilabel(c, &format!("{line} --dry-run"), "")).contains("Would import 1 annotations into 1 of 1 images"));
    let out = ok(oxilabel(c, &line, ""));
    assert!(out.contains("Imported 1 annotations") && out.contains("left out: 1 × label \"dust\" is skipped"), "{out}");

    // Exports: a format into a zip, and an archive with its images, imported again.
    let detect = work.path().join("detect.zip");
    assert!(ok(oxilabel(c, &format!("export --project {p} --format yolo-detect --out {} --include copy --split 100/0/0", detect.display()), "")).contains("Exported 2 images with 1 annotations"));
    let mut magic = [0; 2];
    std::fs::File::open(&detect).unwrap().read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"PK");
    let archive = work.path().join("p.oxilabel.zip");
    ok(oxilabel(c, &format!("export --project {p} --format archive --include copy --out {}", archive.display()), ""));
    assert!(ok(oxilabel(c, &format!("import archive {} --name Copy", archive.display()), "")).contains("\"Copy\" with 2 images and 1 annotations"));

    // Pre-annotation with a threshold, on a model registered through the API.
    api.call("POST", "/api/models", json!({"name": "fake", "adapter": "ultralytics", "weights": "fake.pt"}));
    let out = ok(oxilabel(c, &format!("pre-annotate --project {p} --model fake --threshold 0.5"), ""));
    assert!(out.contains("2 of 2 images, 2 pre-annotations") && out.contains("left out: 2 × less confident than 0.5") && out.contains("Done: 2 pre-annotations"), "{out}");

    // Clear failures with a non-zero exit code.
    let (result, _) = oxilabel(c, "export --project nowhere --format yolo-detect --out x.zip", "");
    assert_eq!((exit_code(&result), result.unwrap_err().0), (1, "no project \"nowhere\" (see `oxilabel projects`)".to_owned()));
    let (result, _) = oxilabel(c, &format!("export --project {p} --format coco --out x.zip"), "");
    assert!(result.unwrap_err().0.contains("no format \"coco\"; there are yolo-detect, yolo-segment, yolo-obb, archive"));
    assert_eq!(exit_code(&oxilabel(c, "export --project 1", "").0), 2, "a usage error");
    assert!(ok(oxilabel(c, "--help", "")).contains("pre-annotate"));

    ok(oxilabel(c, "logout", ""));
    let (result, _) = oxilabel(c, "projects", "");
    assert!(result.unwrap_err().0.starts_with("not signed in"));
}
