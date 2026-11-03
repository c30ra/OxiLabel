use leptos::{prelude::*, task::spawn_local};
use oxilabel_core::ImportReport;

use super::image_pick::{ImagePick, ImagePicker, images_text, import_pick, pick_count};

/// Modal that imports one folder or some images into `project` (ADR 0003): from the server's
/// machine where they are, otherwise uploaded. `on_close` is called when it should disappear.
#[component]
pub fn ImportDialog(
    project: i64,
    project_name: String,
    on_imported: impl Fn() + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let pick = RwSignal::new_local(None::<ImagePick>);
    let count = pick_count(pick);
    let busy = RwSignal::new(None::<String>);
    let outcome = RwSignal::new(None::<Result<ImportReport, String>>);

    let import_label = move || {
        if let Some(step) = busy.get() {
            return step;
        }
        let uploads = pick.with(|pick| pick.as_ref().map(ImagePick::uploads));
        match (uploads, count.get()) {
            (Some(true), Some(Ok(Some(n)))) => format!("Upload {}", images_text(n)),
            (Some(false), Some(Ok(Some(n)))) => format!("Import {}", images_text(n)),
            _ => "Import".to_owned(),
        }
    };

    let start_import = move |_| {
        let Some(chosen) = pick.get_untracked() else { return };
        busy.set(Some("Importing…".into()));
        spawn_local(async move {
            let result = import_pick(project, chosen, move |step| busy.set(Some(step))).await;
            if result.is_ok() {
                pick.set(None);
            }
            // Even a failed upload may have added its earlier batches.
            on_imported();
            outcome.set(Some(result));
            busy.set(None);
        });
    };

    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog import-dialog" role="dialog" aria-modal="true" aria-labelledby="import-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="import-title">{format!("Import images into “{project_name}”")}</h2>
                        <p class="muted">"Choose one folder (subfolders included) or one or more images. On the server's machine they are used where they are; from another computer they are uploaded."</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>

                <Show when=move || outcome.get().is_none() fallback=move || view! { <ImportResult outcome=outcome /> }>
                    <div class="dialog-body">
                        <ImagePicker pick=pick count=count placeholder="Nothing chosen" />
                    </div>
                </Show>

                <div class="dialog-foot">
                    <button on:click=move |_| on_close()>{move || if outcome.get().is_some() { "Close" } else { "Cancel" }}</button>
                    <Show when=move || outcome.get().is_none()>
                        <button class="primary" disabled=move || busy.get().is_some() || pick.with(Option::is_none) on:click=start_import>
                            {import_label}
                        </button>
                    </Show>
                    <Show when=move || outcome.get().is_some()>
                        <button class="primary" on:click=move |_| outcome.set(None)>"Import more"</button>
                    </Show>
                </div>
            </div>
        </div>
    }
}

#[component]
fn ImportResult(outcome: RwSignal<Option<Result<ImportReport, String>>>) -> impl IntoView {
    move || match outcome.get() {
        Some(Err(message)) => {
            view! { <p class="browser-note error" role="alert">{message}</p> }.into_any()
        }
        Some(Ok(report)) => {
            let failed = report.failed.len();
            view! {
                <div class="report" role="status">
                    <p>{format!(
                        "Added {}, skipped {} already in the project, ignored {} non-image files{}.",
                        report.added,
                        report.skipped,
                        report.unsupported,
                        if failed > 0 { format!(", {failed} unreadable") } else { String::new() },
                    )}</p>
                    {(failed > 0).then(|| view! {
                        <ul class="failures">
                            {report.failed.into_iter().map(|failure| view! {
                                <li><code>{failure.path}</code>": "{failure.reason}</li>
                            }).collect_view()}
                        </ul>
                    })}
                </div>
            }.into_any()
        }
        None => ().into_any(),
    }
}
