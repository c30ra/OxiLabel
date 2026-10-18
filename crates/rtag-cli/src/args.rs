//! The command line: `rtag <command> …`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "rtag", version, about = "Import, export and pre-annotate rtag projects from the command line.")]
pub struct Cli {
    /// Where the session is kept (default: ~/.config/rtag).
    #[arg(long, global = true, hide = true)]
    pub config_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Signs in to a server and keeps the session for the other commands.
    Login {
        /// The server's address, e.g. http://localhost:3111.
        #[arg(long)]
        server: String,
        #[arg(long)]
        user: String,
        /// Read the password from the first line of standard input instead of asking for it.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Ends the kept session.
    Logout,
    /// Lists the projects (id, name, images, annotated).
    Projects,
    /// Lists the registered models.
    Models,
    /// Imports a CVAT export into a project, or a project archive as a new project.
    #[command(subcommand)]
    Import(Import),
    Export(Export),
    /// Runs a registered model on a project's images (or a job's) and waits for it, showing progress.
    PreAnnotate(PreAnnotate),
}

#[derive(Debug, Subcommand)]
pub enum Import {
    /// Imports a CVAT "for images 1.1" export (XML or zip) into a project.
    Cvat {
        /// The project, by id or name.
        #[arg(long)]
        project: String,
        file: PathBuf,
        /// Where to look for export images the project lacks (on the server).
        #[arg(long)]
        images_folder: Option<String>,
        /// Replace the annotations of matched images instead of adding to them.
        #[arg(long)]
        replace: bool,
        /// What a CVAT label becomes: CVAT=LABEL (a project label), CVAT=new or CVAT=skip.
        #[arg(long = "label", value_name = "CVAT=WHAT")]
        labels: Vec<String>,
        /// Only report what the import would do.
        #[arg(long)]
        dry_run: bool,
    },
    /// Imports a project archive as a new project.
    Archive {
        file: PathBuf,
        /// The new project's name (default: the archive's).
        #[arg(long)]
        name: Option<String>,
        /// Where to look for images the archive only refers to (on the server).
        #[arg(long)]
        images_folder: Option<String>,
    },
}

/// Exports a project in a format (yolo-detect, yolo-segment, yolo-obb, or any other the server
/// has) or as a project archive, into a zip file.
#[derive(Debug, Args)]
pub struct Export {
    /// The project, by id or name.
    #[arg(long)]
    pub project: String,
    /// An exporter id, or "archive" for a project archive.
    #[arg(long)]
    pub format: String,
    /// The zip file to write.
    #[arg(long)]
    pub out: PathBuf,
    /// Which images: all, annotated (with annotations) or job:N.
    #[arg(long, default_value = "all")]
    pub images: String,
    /// Whether the export includes the image files (a project archive: none or copy).
    #[arg(long, value_enum)]
    pub include: Option<Include>,
    /// Train/val/test percentages, e.g. 80/20/0.
    #[arg(long)]
    pub split: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Include {
    None,
    Symlink,
    Copy,
}

#[derive(Debug, Args)]
pub struct PreAnnotate {
    /// The project, by id or name.
    #[arg(long)]
    pub project: String,
    /// The model, by id or name.
    #[arg(long)]
    pub model: String,
    /// Only the images of this job (its number).
    #[arg(long)]
    pub job: Option<i64>,
    /// Leave out predictions less confident than this (0 to 1).
    #[arg(long)]
    pub threshold: Option<f64>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn parse(line: &str) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(line.split_whitespace())
    }

    #[test]
    fn commands_and_their_options_are_parsed() {
        let cli = parse("rtag login --server http://host:3111 --user anna --password-stdin").unwrap();
        assert!(matches!(cli.command, Command::Login { ref server, ref user, password_stdin: true } if server == "http://host:3111" && user == "anna"));
        let cli = parse("rtag import cvat --project 3 a.zip --label crack=Crack --label dust=skip --dry-run").unwrap();
        let Command::Import(Import::Cvat { project, file, labels, dry_run, replace, .. }) = cli.command else { panic!() };
        assert_eq!((project.as_str(), file, labels, dry_run, replace), ("3", PathBuf::from("a.zip"), vec!["crack=Crack".to_owned(), "dust=skip".to_owned()], true, false));
        let cli = parse("rtag export --project EL --format yolo-obb --out x.zip --images job:2 --include copy --split 70/20/10").unwrap();
        let Command::Export(export) = cli.command else { panic!() };
        assert_eq!((export.images.as_str(), export.include, export.split.as_deref()), ("job:2", Some(Include::Copy), Some("70/20/10")));
        let cli = parse("rtag pre-annotate --project 1 --model yolo --job 4 --threshold 0.3").unwrap();
        let Command::PreAnnotate(run) = cli.command else { panic!() };
        assert_eq!((run.job, run.threshold), (Some(4), Some(0.3)));
    }

    #[test]
    fn mistakes_are_usage_errors() {
        assert!(parse("rtag export --project 1 --format yolo-detect").is_err(), "--out is needed");
        assert!(parse("rtag export --project 1 --format x --out y --include all").is_err());
        assert!(parse("rtag fly").is_err());
    }
}
