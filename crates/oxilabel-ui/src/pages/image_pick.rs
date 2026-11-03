//! Choosing images to import (ADR 0003), shared by the New project and Import images dialogs: one
//! folder or some images, picked in the desktop's dialog on the server's machine and used where
//! they are, or picked in this browser and uploaded.

use leptos::{prelude::*, task::spawn_local};
use oxilabel_core::{FolderCount, ImportReport, ImportRequest, api::BrowseQuery};

use crate::api;

/// Images uploaded per request, so a big folder goes in pieces with progress.
const UPLOAD_BATCH: usize = 16;
const IMAGE_EXTENSIONS: [&str; 7] = ["bmp", "jpeg", "jpg", "png", "tif", "tiff", "webp"];

/// Images to import (ADR 0003): on the server's machine one folder or some files, imported where
/// they are; from this browser a folder's images or some files, uploaded.
#[derive(Clone)]
pub enum ImagePick {
    HostFolder(String),
    HostFiles(Vec<String>),
    Browser { label: String, files: Vec<web_sys::File> },
}

impl ImagePick {
    /// Whether the images are uploaded from this browser rather than used where they are.
    pub fn uploads(&self) -> bool {
        matches!(self, Self::Browser { .. })
    }
}

/// "1 image", "2 images".
pub fn images_text(n: u64) -> String {
    format!("{n} image{}", if n == 1 { "" } else { "s" })
}

/// How many images `pick` holds: counted by the server (`/api/browse/count`) for a host folder,
/// known otherwise; `Ok(None)` without a pick.
pub fn pick_count(pick: RwSignal<Option<ImagePick>, LocalStorage>) -> LocalResource<Result<Option<u64>, String>> {
    LocalResource::new(move || {
        let pick = pick.get();
        async move {
            match pick {
                None => Ok(None),
                Some(ImagePick::HostFolder(path)) => api::get_with_query::<FolderCount>("/api/browse/count", &BrowseQuery { path }.to_query())
                    .await
                    .map(|found| Some(found.images)),
                Some(ImagePick::HostFiles(paths)) => Ok(Some(paths.len() as u64)),
                Some(ImagePick::Browser { files, .. }) => Ok(Some(files.len() as u64)),
            }
        }
    })
}

/// Imports `pick` into `project`: host paths through `/api/projects/{project}/imports`, browser
/// files uploaded to `/api/projects/{project}/uploads` in batches of `UPLOAD_BATCH` with the
/// reports summed. `step` hears what is happening.
pub async fn import_pick(project: i64, pick: ImagePick, step: impl Fn(String)) -> Result<ImportReport, String> {
    let paths = match pick {
        ImagePick::HostFolder(path) => vec![path],
        ImagePick::HostFiles(paths) => paths,
        ImagePick::Browser { files, .. } => return upload(project, &files, step).await,
    };
    step("Importing…".to_owned());
    api::post::<_, ImportReport>(&format!("/api/projects/{project}/imports"), &ImportRequest { paths }).await
}

async fn upload(project: i64, files: &[web_sys::File], step: impl Fn(String)) -> Result<ImportReport, String> {
    let url = format!("/api/projects/{project}/uploads");
    let mut total = ImportReport::default();
    for (done, batch) in files.chunks(UPLOAD_BATCH).enumerate() {
        step(format!("Uploading {} / {}…", done * UPLOAD_BATCH, files.len()));
        let form = web_sys::FormData::new().map_err(|_| "cannot build the upload".to_owned())?;
        for file in batch {
            form.append_with_blob_and_filename("file", file, &relative_path(file)).ok();
        }
        let report = api::post_form::<ImportReport>(&url, form).await?;
        total.added += report.added;
        total.skipped += report.skipped;
        total.unsupported += report.unsupported;
        total.failed.extend(report.failed);
    }
    Ok(total)
}

/// The chosen images, a "Folder…" and an "Images…" button (a new pick replaces the last), and how
/// many images there are.
#[component]
pub fn ImagePicker(
    pick: RwSignal<Option<ImagePick>, LocalStorage>,
    count: LocalResource<Result<Option<u64>, String>>,
    #[prop(into)] placeholder: String,
) -> impl IntoView {
    // The browser's own dialogs, for when the server cannot show one.
    let folder_input = NodeRef::<leptos::html::Input>::new();
    let images_input = NodeRef::<leptos::html::Input>::new();

    // A folder dialog rather than a file one (there is no view! syntax for this attribute).
    Effect::new(move |_| {
        if let Some(input) = folder_input.get() {
            input.set_attribute("webkitdirectory", "").ok();
        }
    });
    let pick_in_browser = move |input: NodeRef<leptos::html::Input>| {
        if let Some(input) = input.get_untracked() {
            // Picking the same files again must still count as a change.
            input.set_value("");
            input.click();
        }
    };
    let choose_folder = move |_| {
        spawn_local(async move {
            // On the server's machine the server shows the desktop's dialog.
            if !api::on_server_machine().await {
                return pick_in_browser(folder_input);
            }
            match api::host_folder_dialog().await {
                Ok(Some(path)) => pick.set(Some(ImagePick::HostFolder(path))),
                Ok(None) => {}
                // No dialog on this server (e.g. headless): the browser's own, with an upload.
                Err(_) => pick_in_browser(folder_input),
            }
        });
    };
    let choose_images = move |_| {
        spawn_local(async move {
            if !api::on_server_machine().await {
                return pick_in_browser(images_input);
            }
            match api::host_images_dialog().await {
                Ok(paths) if !paths.is_empty() => pick.set(Some(ImagePick::HostFiles(paths))),
                Ok(_) => {}
                Err(_) => pick_in_browser(images_input),
            }
        });
    };
    let on_folder = move |ev: leptos::ev::Event| {
        let files = chosen_images(&ev);
        // The picked folder's name, from the files' paths inside it (none if only files came).
        let label = files
            .first()
            .map(relative_path)
            .and_then(|path| path.split_once('/').map(|(folder, _)| format!("{folder}/")))
            .unwrap_or_else(|| "Selected files".to_owned());
        pick.set(Some(ImagePick::Browser { label, files }));
    };
    let on_images = move |ev: leptos::ev::Event| {
        let files = chosen_images(&ev);
        let label = match files.as_slice() {
            [file] => file.name(),
            files => format!("{} images", files.len()),
        };
        pick.set(Some(ImagePick::Browser { label, files }));
    };

    let shown = move || {
        pick.with(|pick| match pick {
            None => String::new(),
            Some(ImagePick::HostFolder(path)) => format!("{}/", path.trim_end_matches('/')),
            Some(ImagePick::HostFiles(paths)) => match paths.as_slice() {
                [path] => path.clone(),
                paths => {
                    let parent = paths.first().and_then(|path| path.rsplit_once('/')).map_or("", |(parent, _)| parent);
                    format!("{} images in {parent}/", paths.len())
                }
            },
            Some(ImagePick::Browser { label, .. }) => label.clone(),
        })
    };
    let count_text = move || {
        pick.with(|pick| match pick {
            None => String::new(),
            Some(ImagePick::Browser { files, .. }) => format!("{} to upload from this browser", images_text(files.len() as u64)),
            Some(ImagePick::HostFiles(paths)) => format!("{} chosen", images_text(paths.len() as u64)),
            Some(ImagePick::HostFolder(_)) => match count.get() {
                Some(Ok(Some(found))) => format!("{} found", images_text(found)),
                Some(Err(message)) => message,
                _ => String::new(),
            },
        })
    };

    view! {
        <div class="image-picker">
            <span class="folder-row">
                <input class="mono" readonly placeholder=placeholder prop:value=shown />
                <Show when=move || pick.with(Option::is_some)>
                    <button type="button" on:click=move |_| pick.set(None)>"Clear"</button>
                </Show>
                <button type="button" class="choose-folder" on:click=choose_folder>"Folder…"</button>
                <button type="button" class="choose-images" on:click=choose_images>"Images…"</button>
            </span>
            <span class="folder-count">{count_text}</span>
            <input type="file" class="pick-folder" node_ref=folder_input multiple hidden on:change=on_folder />
            <input type="file" class="pick-images" node_ref=images_input multiple hidden
                accept=".bmp,.jpeg,.jpg,.png,.tif,.tiff,.webp" on:change=on_images />
        </div>
    }
}

/// The images among the files a file input holds after `ev`.
fn chosen_images(ev: &leptos::ev::Event) -> Vec<web_sys::File> {
    let input: web_sys::HtmlInputElement = event_target(ev);
    let Some(list) = input.files() else { return Vec::new() };
    (0..list.length()).filter_map(|i| list.get(i)).filter(|file| is_image(&file.name())).collect()
}

fn is_image(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// A file's path inside the folder picked in the browser ("folder/sub/name.png").
fn relative_path(file: &web_sys::File) -> String {
    let path = web_sys::js_sys::Reflect::get(file, &"webkitRelativePath".into()).ok().and_then(|path| path.as_string());
    // Empty for files given without their folder.
    path.filter(|path| !path.is_empty()).unwrap_or_else(|| file.name())
}
