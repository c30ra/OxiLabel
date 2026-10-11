use leptos::{prelude::*, task::spawn_local};
use rtag_core::{ImagePage, ImportReport, ImportRequest};

use crate::api;

const PAGE_SIZE: i64 = 60;

/// Import form plus a paged grid of the project's image thumbnails.
#[component]
pub fn ImageSet(project: i64) -> impl IntoView {
    let offset = RwSignal::new(0_i64);
    let images = LocalResource::new(move || {
        let offset = offset.get();
        async move {
            api::get::<ImagePage>(&format!("/api/projects/{project}/images?offset={offset}&limit={PAGE_SIZE}")).await
        }
    });

    view! {
        <h2>"Images"</h2>
        <ImportForm project on_imported=move || images.refetch() />
        {move || match images.get() {
            None => view! { <p>"Loading…"</p> }.into_any(),
            Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
            Some(Ok(page)) if page.total == 0 => view! { <p>"No images yet."</p> }.into_any(),
            Some(Ok(page)) => {
                let total = page.total;
                let first = offset.get_untracked() + 1;
                let last = offset.get_untracked() + page.images.len() as i64;
                view! {
                    <div class="pager">
                        <button disabled=move || offset.get() == 0
                            on:click=move |_| offset.update(|o| *o = (*o - PAGE_SIZE).max(0))>"‹ Previous"</button>
                        <span>{format!("{first}–{last} of {total}")}</span>
                        <button disabled=move || offset.get() + PAGE_SIZE >= total
                            on:click=move |_| offset.update(|o| *o += PAGE_SIZE)>"Next ›"</button>
                    </div>
                    <ul class="grid">
                        {page.images.into_iter().map(|image| {
                            let name = image.path.rsplit('/').next().unwrap_or_default().to_owned();
                            view! {
                                <li title=image.path.clone()>
                                    <img loading="lazy" alt="" src=format!("/api/images/{}/thumbnail", image.id) />
                                    <span class="caption">
                                        <span class=if image.annotated { "status done" } else { "status" }
                                            title=if image.annotated { "Annotated" } else { "Not annotated" }></span>
                                        {name}
                                    </span>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any()
            }
        }}
    }
}

#[component]
fn ImportForm(project: i64, on_imported: impl Fn() + Copy + 'static) -> impl IntoView {
    let folder = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let result = RwSignal::new(None::<Result<ImportReport, String>>);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let request = ImportRequest { folder: folder.get() };
        busy.set(true);
        result.set(None);
        spawn_local(async move {
            let report = api::post::<_, ImportReport>(&format!("/api/projects/{project}/imports"), &request).await;
            if report.is_ok() {
                on_imported();
            }
            result.set(Some(report));
            busy.set(false);
        });
    };

    view! {
        <form class="row" on:submit=on_submit>
            <input placeholder="Folder inside the image root (empty imports the whole root)"
                prop:value=folder on:input:target=move |ev| folder.set(ev.target().value()) />
            <button type="submit" disabled=busy>{move || if busy.get() { "Importing…" } else { "Import" }}</button>
        </form>
        {move || result.get().map(|outcome| match outcome {
            Err(message) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
            Ok(report) => {
                let failed = report.failed.len();
                view! {
                    <div class="report" role="status">
                        <p>{format!(
                            "Added {}, skipped {} already known, ignored {} non-image files{}.",
                            report.added,
                            report.skipped,
                            report.unsupported,
                            if failed > 0 { format!(", {failed} unreadable") } else { String::new() },
                        )}</p>
                        {(failed > 0).then(|| view! {
                            <details>
                                <summary>"Unreadable files"</summary>
                                <ul>
                                    {report.failed.into_iter().map(|failure| view! {
                                        <li><code>{failure.path}</code>": "{failure.reason}</li>
                                    }).collect_view()}
                                </ul>
                            </details>
                        })}
                    </div>
                }.into_any()
            }
        })}
    }
}
