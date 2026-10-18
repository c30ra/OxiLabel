//! Moving through a project's images in the workspace: the image's position, the filmstrip of
//! its neighbours with their status, the annotated status itself, and the keys: j next, k
//! previous, gg first, G last (vim motions), Ctrl+Enter marks the image annotated and opens the
//! next one.

use leptos::{ev, prelude::*, task::spawn_local};
use leptos_router::hooks::use_navigate;
use rtag_core::{Image, ImageNeighbours, ImageStatus};

use rtag_core::keymap::Action;

use crate::{
    annotate::Editor,
    api,
    prefs::{prefs, press},
    viewer::typing_in_a_field,
};

/// Images on each side of the current one in the filmstrip.
const NEIGHBOURS: usize = 6;
/// The two presses of gg must come this close together (milliseconds).
const DOUBLE_KEY: f64 = 800.0;

type OpenUrl = Box<dyn Fn(&str)>;

/// Where the open image is among its project's images, shared by the header and the filmstrip.
#[derive(Clone, Copy)]
pub struct ImageNav {
    project: i64,
    image: i64,
    /// Every image id of the project, in path order.
    ids: LocalResource<Result<Vec<i64>, String>>,
    neighbours: LocalResource<Result<ImageNeighbours, String>>,
    pub annotated: RwSignal<bool>,
    /// Bumped when a status changes, to show it in the filmstrip.
    changed: RwSignal<u32>,
    /// Opens another image (the router's navigation, taken while the workspace is built).
    open: StoredValue<OpenUrl, LocalStorage>,
}

impl ImageNav {
    pub fn new(project: i64, image: &Image) -> Self {
        let id = image.id;
        let changed = RwSignal::new(0_u32);
        ImageNav {
            project,
            image: id,
            ids: LocalResource::new(move || async move { api::get::<Vec<i64>>(&format!("/api/projects/{project}/image-ids")).await }),
            neighbours: LocalResource::new(move || {
                changed.track();
                async move { api::get::<ImageNeighbours>(&format!("/api/projects/{project}/images/{id}/neighbours?count={NEIGHBOURS}")).await }
            }),
            annotated: RwSignal::new(image.annotated),
            changed,
            open: {
                let navigate = use_navigate();
                StoredValue::new_local(Box::new(move |url: &str| navigate(url, Default::default())) as OpenUrl)
            },
        }
    }

    /// Opens the image `step` places away (see `target`), if there is one.
    pub fn go(&self, step: i64) {
        if let Some(target) = self.target(step) {
            self.open.with_value(|open| open(&self.url(target)));
        }
    }

    /// The image `step` places away (negative is back), the first (`i64::MIN`) or the last
    /// (`i64::MAX`), if there is one.
    fn target(&self, step: i64) -> Option<i64> {
        let ids = self.ids.get_untracked()?.ok()?;
        let here = ids.iter().position(|&id| id == self.image)? as i64;
        let index = match step {
            i64::MIN => 0,
            i64::MAX => ids.len() as i64 - 1,
            step => here + step,
        };
        usize::try_from(index).ok().and_then(|index| ids.get(index).copied()).filter(|&id| id != self.image)
    }

    pub fn url(&self, image: i64) -> String {
        format!("/projects/{}/images/{image}", self.project)
    }

    /// Sets the status of the open image; with `then_next`, opens the next image afterwards.
    pub fn set_annotated(self, annotated: bool, then_next: bool, editor: Editor) {
        let url = format!("/api/projects/{}/images/{}/status", self.project, self.image);
        spawn_local(async move {
            match api::put::<_, Image>(&url, &ImageStatus { annotated }).await {
                Ok(image) => {
                    self.annotated.set(image.annotated);
                    self.changed.update(|n| *n = n.wrapping_add(1));
                    if then_next {
                        let loaded = self.ids.get_untracked().is_some_and(|ids| ids.is_ok());
                        match self.target(1) {
                            Some(_) => self.go(1),
                            None if loaded => editor.notice.set(Some("Marked annotated. That was the last image.".into())),
                            None => {}
                        }
                    }
                }
                Err(message) => editor.notice.set(Some(message)),
            }
        });
    }
}

/// "12 / 451" and the status button, for the header.
#[component]
pub fn ImagePosition(nav: ImageNav, editor: Editor) -> impl IntoView {
    let position = move || {
        let ids = nav.ids.get()?.ok()?;
        let here = ids.iter().position(|&id| id == nav.image)?;
        Some(format!("{} / {}", here + 1, ids.len()))
    };
    view! {
        <span class="image-position mono">{position}</span>
        <button class="mark-annotated" class:done=move || nav.annotated.get()
            title="Toggle the image's status (Ctrl+Enter marks it annotated and opens the next one)"
            on:click=move |_| nav.set_annotated(!nav.annotated.get_untracked(), false, editor)>
            {move || if nav.annotated.get() { "Annotated ✓" } else { "Mark annotated" }}
            // Ctrl+Enter marks, it never un-marks: the hint only goes with marking.
            <Show when=move || !nav.annotated.get()><kbd>"Ctrl ↵"</kbd></Show>
        </button>
    }
}

/// The neighbouring images with their status, and the keys that move through them.
#[component]
pub fn Filmstrip(nav: ImageNav, editor: Editor) -> impl IntoView {
    let go = move |step: i64| nav.go(step);
    let last_g = StoredValue::new(f64::NEG_INFINITY);
    let keymap = prefs().keymap;
    let keys = window_event_listener(ev::keydown, move |ev| {
        // Keys the editor took (e.g. a label bound to j), held keys, open dialogs and pies, and
        // shapes being drawn leave the image alone.
        let open = document().query_selector(".backdrop, .pie, .label-search").ok().flatten().is_some();
        if ev.default_prevented() || ev.repeat() || typing_in_a_field() || open || editor.busy.get_untracked() {
            return;
        }
        let keys = keymap.get_untracked();
        let press = press(&ev);
        if keys.matches(Action::MarkAnnotated, &press) {
            ev.prevent_default();
            nav.set_annotated(true, true, editor);
            return;
        }
        if keys.matches(Action::NextImage, &press) {
            go(1);
        } else if keys.matches(Action::PreviousImage, &press) {
            go(-1);
        } else if keys.matches(Action::LastImage, &press) {
            go(i64::MAX);
        } else if keys.matches(Action::FirstImage, &press) {
            // Pressed twice in a row (gg).
            let now = web_sys::js_sys::Date::now();
            if now - last_g.get_value() <= DOUBLE_KEY {
                last_g.set_value(f64::NEG_INFINITY);
                go(i64::MIN);
            } else {
                last_g.set_value(now);
            }
            ev.prevent_default();
            return;
        } else {
            last_g.set_value(f64::NEG_INFINITY);
            return;
        }
        last_g.set_value(f64::NEG_INFINITY);
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    view! {
        <nav class="filmstrip" aria-label="Images of the project">
            <button class="step" aria-label="Previous image (k)" title="Previous image (k)" on:click=move |_| go(-1)>"‹"</button>
            {move || match nav.neighbours.get() {
                Some(Ok(around)) => around.images.into_iter().map(|image| {
                    let current = image.id == nav.image;
                    let annotated = if current { nav.annotated.get() } else { image.annotated };
                    let name = image.path.rsplit('/').next().unwrap_or_default().to_owned();
                    view! {
                        <a class="thumb" class:current=current class:annotated=annotated href=nav.url(image.id) title=name.clone() aria-label=name>
                            <img src=format!("/api/images/{}/thumbnail", image.id) alt="" loading="lazy" />
                            <span class="dot" title=if annotated { "Annotated" } else { "Not annotated" }></span>
                        </a>
                    }
                }).collect_view().into_any(),
                Some(Err(message)) => view! { <span class="error">{message}</span> }.into_any(),
                None => view! { <span class="muted">"…"</span> }.into_any(),
            }}
            <button class="step" aria-label="Next image (j)" title="Next image (j)" on:click=move |_| go(1)>"›"</button>
        </nav>
    }
}
