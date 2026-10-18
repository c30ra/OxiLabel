//! rtag's command-line client: the server's operations (import, export, pre-annotation) from a
//! shell. It signs in once (`rtag login`) and keeps the session cookie in its config folder; every
//! command then calls the server's HTTP API, as the browser does.

pub mod args;

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use args::{Cli, Command, Export, Import, Include, PreAnnotate};
use reqwest::{
    StatusCode,
    blocking::{Client as Http, RequestBuilder, Response, multipart},
};
use rtag_core::{
    Label, ProjectSummary,
    export::{ExportFormat, ExportRequest, ExportResult, ImageMode, ImageSelection, Split},
    import::{ArchiveImportPlan, ArchiveImportReport, CvatImportPlan, CvatImportReport, LabelChoice},
    inference::{Model, RunRequest, RunState, RunStatus},
    job::Job,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// A failure, told to the user as one line; `usage` for a command line that does not parse.
#[derive(Debug)]
pub struct CliError(pub String, pub bool);

impl CliError {
    fn usage(&self) -> bool {
        self.1
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How the program ends: 0 done, 1 failed, 2 a command line it does not understand.
pub fn exit_code(result: &Result<(), CliError>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(err) if err.usage() => 2,
        Err(_) => 1,
    }
}

fn fail<T>(message: impl Into<String>) -> Result<T, CliError> {
    Err(CliError(message.into(), false))
}

/// How often a running pre-annotation is asked for its progress.
const POLL: Duration = Duration::from_millis(500);

/// The kept session: which server, and the cookie it gave.
#[derive(Serialize, Deserialize)]
struct Session {
    server: String,
    cookie: String,
}

/// The default config folder: `$XDG_CONFIG_HOME/rtag`, else `~/.config/rtag`.
pub fn default_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rtag")
}

struct Client {
    server: String,
    cookie: String,
    http: Http,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

impl Client {
    fn open(config: &Path) -> Result<Self, CliError> {
        let text = fs::read_to_string(config.join("session")).map_err(|_| CliError("not signed in: run `rtag login --server URL --user NAME` first".into(), false))?;
        let session: Session = serde_json::from_str(&text).map_err(|_| CliError("the kept session is unreadable: run `rtag login` again".into(), false))?;
        Ok(Client { server: session.server, cookie: session.cookie, http: http()? })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }

    /// Sends the request with the session and turns a refusal into the server's message.
    fn send(&self, request: RequestBuilder) -> Result<Response, CliError> {
        let response = request.header("cookie", &self.cookie).send().map_err(|err| CliError(format!("cannot reach {}: {err}", self.server), false))?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let message = response.json::<ErrorBody>().map(|body| body.error).unwrap_or_else(|_| format!("the server answered {status}"));
        match status {
            StatusCode::UNAUTHORIZED => fail(format!("{message} (the session ended: run `rtag login` again)")),
            _ => fail(message),
        }
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, CliError> {
        read(self.send(self.http.get(self.url(path)))?)
    }

    fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T, CliError> {
        read(self.send(self.http.post(self.url(path)).json(body))?)
    }

    fn post_form<T: DeserializeOwned>(&self, path: &str, form: multipart::Form) -> Result<T, CliError> {
        read(self.send(self.http.post(self.url(path)).multipart(form))?)
    }

    /// Writes what `path` answers into the file at `out`.
    fn download(&self, path: &str, out: &Path) -> Result<u64, CliError> {
        let mut response = self.send(self.http.get(self.url(path)))?;
        let mut file = fs::File::create(out).map_err(|err| CliError(format!("cannot write {}: {err}", out.display()), false))?;
        response.copy_to(&mut file).map_err(|err| CliError(format!("the download broke off: {err}"), false))
    }

    /// A project by id or by name.
    fn project(&self, wanted: &str) -> Result<ProjectSummary, CliError> {
        let projects: Vec<ProjectSummary> = self.get("/api/projects")?;
        let found = match wanted.parse::<i64>() {
            Ok(id) => projects.into_iter().find(|p| p.id == id),
            Err(_) => {
                let named: Vec<_> = projects.into_iter().filter(|p| p.name.eq_ignore_ascii_case(wanted)).collect();
                if named.len() > 1 {
                    return fail(format!("several projects are named \"{wanted}\": give its id (see `rtag projects`)"));
                }
                named.into_iter().next()
            }
        };
        found.ok_or_else(|| CliError(format!("no project \"{wanted}\" (see `rtag projects`)"), false))
    }

    /// A job's id, by its number in the project.
    fn job(&self, project: i64, number: i64) -> Result<i64, CliError> {
        let jobs: Vec<Job> = self.get(&format!("/api/projects/{project}/jobs"))?;
        jobs.into_iter().find(|job| job.number == number).map(|job| job.id).ok_or_else(|| CliError(format!("the project has no job {number}"), false))
    }
}

fn http() -> Result<Http, CliError> {
    // No timeout: exports and imports of big projects take as long as they take.
    Http::builder().timeout(None).build().map_err(|err| CliError(err.to_string(), false))
}

fn read<T: DeserializeOwned>(response: Response) -> Result<T, CliError> {
    response.json().map_err(|err| CliError(format!("the server's answer is unreadable: {err}"), false))
}

fn file_part(path: &Path) -> Result<multipart::Part, CliError> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "upload".into());
    let file = fs::File::open(path).map_err(|err| CliError(format!("cannot read {}: {err}", path.display()), false))?;
    Ok(multipart::Part::reader(file).file_name(name))
}

/// Runs a command line (`args` with the program name first), writing what it does to `out`.
/// `input` gives the password for `login --password-stdin`.
pub fn run(args: impl IntoIterator<Item = String>, out: &mut dyn Write, input: &mut dyn BufRead) -> Result<(), CliError> {
    let cli = match <Cli as clap::Parser>::try_parse_from(args) {
        Ok(cli) => cli,
        // Help and the version are answers, not failures.
        Err(err) if matches!(err.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) => {
            return write!(out, "{err}").map_err(|err| CliError(err.to_string(), false));
        }
        Err(err) => return Err(CliError(err.to_string(), true)),
    };
    let config = cli.config_dir.clone().unwrap_or_else(default_config_dir);
    let say = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|err| CliError(err.to_string(), false));
    match cli.command {
        Command::Login { server, user, password_stdin } => {
            let server = server.trim_end_matches('/').to_owned();
            let password = if password_stdin {
                let mut line = String::new();
                input.read_line(&mut line).map_err(|err| CliError(err.to_string(), false))?;
                line.trim_end_matches(['\r', '\n']).to_owned()
            } else {
                rpassword::prompt_password(format!("Password for {user} on {server}: ")).map_err(|err| CliError(format!("cannot read the password: {err}"), false))?
            };
            let response = http()?
                .post(format!("{server}/api/login"))
                .json(&serde_json::json!({"username": user, "password": password}))
                .send()
                .map_err(|err| CliError(format!("cannot reach {server}: {err}"), false))?;
            if !response.status().is_success() {
                let message = response.json::<ErrorBody>().map(|body| body.error).unwrap_or_else(|_| "the sign-in was refused".into());
                return fail(message);
            }
            let cookie = response
                .headers()
                .get("set-cookie")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .ok_or_else(|| CliError("the server gave no session".into(), false))?
                .to_owned();
            write_session(&config, &Session { server: server.clone(), cookie })?;
            say(out, format!("Signed in to {server} as {user}."))
        }
        Command::Logout => {
            if let Ok(client) = Client::open(&config) {
                // The session ends on the server too; a server that is gone needs nothing.
                let _ = client.send(client.http.post(client.url("/api/logout")));
            }
            fs::remove_file(config.join("session")).ok();
            say(out, "Signed out.".into())
        }
        Command::Projects => {
            let client = Client::open(&config)?;
            for project in client.get::<Vec<ProjectSummary>>("/api/projects")? {
                say(out, format!("{:>4}  {}  ({} images, {} annotated, {} jobs)", project.id, project.name, project.images, project.annotated, project.jobs))?;
            }
            Ok(())
        }
        Command::Models => {
            let client = Client::open(&config)?;
            for model in client.get::<Vec<Model>>("/api/models")? {
                say(out, format!("{:>4}  {}  ({} · {})", model.id, model.definition.name, model.definition.adapter, model.definition.weights))?;
            }
            Ok(())
        }
        Command::Import(Import::Cvat { project, file, images_folder, replace, labels, dry_run }) => {
            let client = Client::open(&config)?;
            let project = client.project(&project)?;
            let existing: Vec<Label> = client.get(&format!("/api/projects/{}/labels", project.id))?;
            let mut choices = BTreeMap::new();
            for mapping in labels {
                let (cvat, what) = mapping.split_once('=').ok_or_else(|| CliError(format!("--label {mapping}: write CVAT=LABEL, CVAT=new or CVAT=skip"), false))?;
                let choice = match what {
                    "new" => LabelChoice::Create,
                    "skip" => LabelChoice::Skip,
                    name => LabelChoice::Map {
                        label: existing.iter().find(|label| label.name.eq_ignore_ascii_case(name)).map(|label| label.id).ok_or_else(|| CliError(format!("--label {mapping}: the project has no label \"{name}\""), false))?,
                    },
                };
                choices.insert(cvat.to_owned(), choice);
            }
            let plan = CvatImportPlan { labels: choices, replace, commit: !dry_run, images_folder };
            let form = multipart::Form::new().part("file", file_part(&file)?).text("plan", serde_json::to_string(&plan).expect("plans serialise"));
            let report: CvatImportReport = client.post_form(&format!("/api/projects/{}/cvat-imports", project.id), form)?;
            for label in &report.labels {
                let what = match label.choice {
                    LabelChoice::Map { label: id } => format!("→ {}", existing.iter().find(|l| l.id == id).map_or("?", |l| l.name.as_str())),
                    LabelChoice::Create => "new label".into(),
                    LabelChoice::Skip => "left out".into(),
                };
                say(out, format!("  {}: {} shapes, {what}{}", label.name, label.annotations, label.problem.as_ref().map(|p| format!(" ({p})")).unwrap_or_default()))?;
            }
            for (list, what) in [(&report.unmatched, "not in the project"), (&report.ambiguous, "matching several images"), (&report.size_mismatch, "of another size")] {
                if !list.is_empty() {
                    say(out, format!("  {} images {what}: {}", list.len(), list.join(", ")))?;
                }
            }
            for skip in &report.skipped {
                say(out, format!("  left out: {} × {}", skip.count, skip.reason))?;
            }
            let verb = if report.committed { "Imported" } else { "Would import" };
            say(out, format!("{verb} {} annotations into {} of {} images ({} added to the project).", report.annotations, report.matched, report.images, report.added))
        }
        Command::Import(Import::Archive { file, name, images_folder }) => {
            let client = Client::open(&config)?;
            let plan = ArchiveImportPlan { name, images_folder };
            let form = multipart::Form::new().text("plan", serde_json::to_string(&plan).expect("plans serialise")).part("file", file_part(&file)?);
            let report: ArchiveImportReport = client.post_form("/api/project-archives", form)?;
            if !report.missing.is_empty() {
                say(out, format!("  {} images not found, left out: {}", report.missing.len(), report.missing.join(", ")))?;
            }
            if !report.unassigned_jobs.is_empty() {
                say(out, format!("  unassigned (no such user here): jobs {:?}", report.unassigned_jobs))?;
            }
            for skip in &report.skipped {
                say(out, format!("  left out: {} × {}", skip.count, skip.reason))?;
            }
            say(out, format!("Made project {} \"{}\" with {} images and {} annotations.", report.project.id, report.project.name, report.images, report.annotations))
        }
        Command::Export(export) => run_export(&Client::open(&config)?, export, out),
        Command::PreAnnotate(request) => run_pre_annotation(&Client::open(&config)?, request, out),
    }
}

fn write_session(config: &Path, session: &Session) -> Result<(), CliError> {
    fs::create_dir_all(config).map_err(|err| CliError(format!("cannot keep the session in {}: {err}", config.display()), false))?;
    let path = config.join("session");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // The cookie signs in as the user: only they may read it.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(|err| CliError(format!("cannot keep the session in {}: {err}", path.display()), false))?;
    file.write_all(serde_json::to_string(session).expect("sessions serialise").as_bytes()).map_err(|err| CliError(err.to_string(), false))
}

fn run_export(client: &Client, export: Export, out: &mut dyn Write) -> Result<(), CliError> {
    let project = client.project(&export.project)?;
    if export.format == "archive" {
        let images = match export.include {
            None | Some(Include::None) => false,
            Some(Include::Copy) => true,
            Some(Include::Symlink) => return fail("a project archive includes images as copies or not at all (--include copy or none)"),
        };
        let bytes = client.download(&format!("/api/projects/{}/archive?images={images}", project.id), &export.out)?;
        return writeln!(out, "Wrote the archive of \"{}\" to {} ({} bytes).", project.name, export.out.display(), bytes).map_err(|err| CliError(err.to_string(), false));
    }
    let formats: Vec<ExportFormat> = client.get("/api/exporters")?;
    if !formats.iter().any(|format| format.id == export.format) {
        let known: Vec<&str> = formats.iter().map(|format| format.id.as_str()).chain(["archive"]).collect();
        return fail(format!("no format \"{}\"; there are {}", export.format, known.join(", ")));
    }
    let images = match export.images.as_str() {
        "all" => ImageSelection::All,
        "annotated" => ImageSelection::WithAnnotations,
        other => match other.strip_prefix("job:").and_then(|number| number.parse().ok()) {
            Some(number) => {
                let job = client.job(project.id, number)?;
                ImageSelection::Ids { ids: client.get(&format!("/api/projects/{}/image-ids?job={job}", project.id))? }
            }
            None => return fail(format!("--images {other}: write all, annotated or job:N")),
        },
    };
    let split = match &export.split {
        None => Split::default(),
        Some(text) => {
            let parts: Vec<u8> = text.split('/').map(|part| part.trim().parse()).collect::<Result<_, _>>().map_err(|_| CliError(format!("--split {text}: write train/val/test percentages, e.g. 80/20/0"), false))?;
            let [train, val, test] = parts[..] else { return fail(format!("--split {text}: give three percentages")) };
            Split { train, val, test }
        }
    };
    let include_images = export.include.map(|include| match include {
        Include::None => ImageMode::None,
        Include::Symlink => ImageMode::Symlink,
        Include::Copy => ImageMode::Copy,
    });
    let request = ExportRequest { format: export.format.clone(), images, include_images, split, footprints: Default::default(), footprint_bounds: true };
    let result: ExportResult = client.post(&format!("/api/projects/{}/exports", project.id), &request)?;
    client.download(&result.download, &export.out)?;
    writeln!(out, "Exported {} images with {} annotations ({}) to {}.", result.image_count, result.annotation_count, export.format, export.out.display()).map_err(|err| CliError(err.to_string(), false))
}

fn run_pre_annotation(client: &Client, request: PreAnnotate, out: &mut dyn Write) -> Result<(), CliError> {
    let project = client.project(&request.project)?;
    let models: Vec<Model> = client.get("/api/models")?;
    let model = models
        .iter()
        .find(|model| request.model.parse::<i64>().is_ok_and(|id| id == model.id) || model.definition.name.eq_ignore_ascii_case(&request.model))
        .ok_or_else(|| CliError(format!("no model \"{}\" (see `rtag models`)", request.model), false))?;
    let (image_ids, job) = match request.job {
        Some(number) => (Vec::new(), Some(client.job(project.id, number)?)),
        None => (client.get(&format!("/api/projects/{}/image-ids", project.id))?, None),
    };
    let run = RunRequest { model: model.id, image_ids, job, threshold: request.threshold };
    let mut status: RunStatus = client.post(&format!("/api/projects/{}/pre-annotations", project.id), &run)?;
    let say = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|err| CliError(err.to_string(), false));
    let mut shown = usize::MAX;
    loop {
        if status.done != shown {
            shown = status.done;
            say(out, format!("{} of {} images, {} pre-annotations", status.done, status.total, status.created))?;
        }
        if status.state != RunState::Running {
            break;
        }
        thread::sleep(POLL);
        status = client.get(&format!("/api/runs/{}", status.id))?;
    }
    for skip in &status.skipped {
        say(out, format!("  left out: {} × {}", skip.count, skip.reason))?;
    }
    match status.state {
        RunState::Finished => say(out, format!("Done: {} pre-annotations from {} to review.", status.created, status.model)),
        RunState::Cancelled => fail(format!("the run was cancelled after {} of {} images", status.done, status.total)),
        _ => fail(format!("the run failed: {}", status.error.unwrap_or_default())),
    }
}
