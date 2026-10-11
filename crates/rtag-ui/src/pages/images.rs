use leptos::prelude::*;
use rtag_core::ImagePage;

use crate::api;

const PAGE_SIZE: i64 = 60;

/// Paged grid of the project's image thumbnails. `version` changes whenever images were added.
#[component]
pub fn ImageSet(project: i64, version: RwSignal<u32>) -> impl IntoView {
    let offset = RwSignal::new(0_i64);
    let images = LocalResource::new(move || {
        let offset = offset.get();
        version.track();
        async move {
            api::get::<ImagePage>(&format!("/api/projects/{project}/images?offset={offset}&limit={PAGE_SIZE}")).await
        }
    });

    move || match images.get() {
        None => view! { <p class="muted">"Loading…"</p> }.into_any(),
        Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
        Some(Ok(page)) if page.total == 0 => {
            view! { <p class="muted">"No images yet. Use “Import images…” to add some from the server's image folder."</p> }
                .into_any()
        }
        Some(Ok(page)) => {
            let total = page.total;
            let first = offset.get_untracked() + 1;
            let last = offset.get_untracked() + page.images.len() as i64;
            view! {
                <div class="pager">
                    <button class="icon" aria-label="Previous page" disabled=move || offset.get() == 0
                        on:click=move |_| offset.update(|o| *o = (*o - PAGE_SIZE).max(0))>"‹"</button>
                    <span class="mono">{format!("{first}–{last} of {total}")}</span>
                    <button class="icon" aria-label="Next page" disabled=move || offset.get() + PAGE_SIZE >= total
                        on:click=move |_| offset.update(|o| *o += PAGE_SIZE)>"›"</button>
                </div>
                <ul class="grid">
                    {page.images.into_iter().map(|image| {
                        let name = image.path.rsplit('/').next().unwrap_or_default().to_owned();
                        view! {
                            <li title=image.path.clone()>
                                <img loading="lazy" alt="" src=format!("/api/images/{}/thumbnail", image.id) />
                                <span class="caption">
                                    <span class=if image.annotated { "status-dot done" } else { "status-dot" }
                                        title=if image.annotated { "Annotated" } else { "Not annotated" }></span>
                                    <span class="name">{name}</span>
                                </span>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            }
            .into_any()
        }
    }
}
