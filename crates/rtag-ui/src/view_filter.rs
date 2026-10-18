//! The view filter panel over the image (as in the approved workspace design): an enhancement
//! filter preset with its parameters, computed by the server, and brightness and contrast applied
//! in the browser on top. Holding O shows the original. The settings are remembered per project in
//! this browser.

use std::{collections::HashMap, time::Duration};

use leptos::{ev, prelude::*};
use rtag_core::filter::{Filter, Preset};
use serde::{Deserialize, Serialize};

use rtag_core::keymap::Action;

use crate::{
    prefs::{prefs, press},
    viewer::typing_in_a_field,
};

/// How long the parameters must rest before the server is asked for the filtered view.
const APPLY_DELAY: Duration = Duration::from_millis(300);

/// The view filter settings of a project's workspace.
#[derive(Clone, Copy)]
pub struct ViewFilter {
    /// The chosen preset; `None` is the original image.
    pub preset: RwSignal<Option<Preset>>,
    /// The parameters of every preset, so switching back keeps them.
    filters: RwSignal<Vec<Filter>>,
    /// In percent; 100 leaves the image as it is.
    pub brightness: RwSignal<f64>,
    pub contrast: RwSignal<f64>,
    pub collapsed: RwSignal<bool>,
    /// Whether O is held: the original is shown.
    pub original: RwSignal<bool>,
    /// The filter the viewer shows (its key), following the settings once they rest.
    pub applied: RwSignal<Option<String>>,
    /// Whether the viewer is still waiting for tiles of the applied filter.
    pub loading: RwSignal<bool>,
}

/// What is remembered, in local storage. Nothing saved yet: the original, panel collapsed.
#[derive(Serialize, Deserialize)]
struct Saved {
    preset: Option<String>,
    values: HashMap<String, Vec<f64>>,
    brightness: Option<f64>,
    contrast: Option<f64>,
    collapsed: bool,
}

impl Default for Saved {
    fn default() -> Saved {
        Saved { preset: None, values: HashMap::new(), brightness: None, contrast: None, collapsed: true }
    }
}

fn storage_key(project: i64) -> String {
    format!("rtag.view-filter.{project}")
}

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

impl ViewFilter {
    /// The settings of `project` as last left in this browser.
    pub fn new(project: i64) -> ViewFilter {
        let saved: Saved = storage()
            .and_then(|storage| storage.get_item(&storage_key(project)).ok().flatten())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let preset = Preset::ALL.into_iter().find(|preset| saved.preset.as_deref() == Some(preset.name()));
        let filters = Preset::ALL
            .into_iter()
            .map(|preset| match saved.values.get(preset.name()) {
                Some(values) => Filter::with_values(preset, values),
                None => Filter::new(preset),
            })
            .collect();
        let filter = ViewFilter {
            preset: RwSignal::new(preset),
            filters: RwSignal::new(filters),
            brightness: RwSignal::new(saved.brightness.unwrap_or(100.0).clamp(0.0, 300.0)),
            contrast: RwSignal::new(saved.contrast.unwrap_or(100.0).clamp(0.0, 300.0)),
            collapsed: RwSignal::new(saved.collapsed),
            original: RwSignal::new(false),
            applied: RwSignal::new(None),
            loading: RwSignal::new(false),
        };
        filter.applied.set(filter.key_untracked());

        // Remember every change.
        Effect::new(move |_| {
            let saved = Saved {
                preset: filter.preset.get().map(|preset| preset.name().to_owned()),
                values: filter.filters.with(|filters| filters.iter().map(|f| (f.preset().name().to_owned(), f.values().to_vec())).collect()),
                brightness: Some(filter.brightness.get()),
                contrast: Some(filter.contrast.get()),
                collapsed: filter.collapsed.get(),
            };
            if let (Some(storage), Ok(text)) = (storage(), serde_json::to_string(&saved)) {
                storage.set_item(&storage_key(project), &text).ok();
            }
        });
        // Apply the settings once they rest.
        let changes = StoredValue::new(0_u64);
        Effect::new(move |first: Option<()>| {
            let key = filter.key();
            if first.is_none() {
                return;
            }
            changes.update_value(|n| *n += 1);
            let change = changes.get_value();
            set_timeout(
                move || {
                    if changes.get_value() == change {
                        filter.applied.set(key);
                    }
                },
                APPLY_DELAY,
            );
        });
        filter
    }

    /// The chosen filter's key, as soon as it changes.
    fn key(&self) -> Option<String> {
        let preset = self.preset.get()?;
        self.filters.with(|filters| filters.iter().find(|f| f.preset() == preset).map(Filter::key))
    }

    fn key_untracked(&self) -> Option<String> {
        let preset = self.preset.get_untracked()?;
        self.filters.with_untracked(|filters| filters.iter().find(|f| f.preset() == preset).map(Filter::key))
    }

    /// The filter whose tiles are on screen: none while O is held.
    pub fn shown(&self) -> Option<String> {
        if self.original.get() { None } else { self.applied.get() }
    }

    /// The CSS filter of the image for brightness and contrast.
    pub fn css(&self) -> String {
        let (brightness, contrast) = (self.brightness.get(), self.contrast.get());
        if self.original.get() || (brightness == 100.0 && contrast == 100.0) {
            "none".to_owned()
        } else {
            format!("brightness({brightness}%) contrast({contrast}%)")
        }
    }

    /// What the view shows, for the header.
    pub fn title(&self) -> &'static str {
        match (self.original.get(), self.preset.get()) {
            (false, Some(preset)) => preset.title(),
            _ => "Original",
        }
    }

    fn value(&self, preset: Preset, index: usize) -> f64 {
        self.filters.with(|filters| filters.iter().find(|f| f.preset() == preset).map_or(0.0, |f| f.values()[index]))
    }

    fn set_value(&self, preset: Preset, index: usize, value: f64) {
        self.filters.update(|filters| {
            if let Some(filter) = filters.iter_mut().find(|f| f.preset() == preset) {
                filter.set(index, value);
            }
        });
    }

    /// Back to the defaults of the current preset, and no brightness or contrast change.
    fn reset(&self) {
        if let Some(preset) = self.preset.get_untracked() {
            self.filters.update(|filters| {
                if let Some(filter) = filters.iter_mut().find(|f| f.preset() == preset) {
                    *filter = Filter::new(preset);
                }
            });
        }
        self.brightness.set(100.0);
        self.contrast.set(100.0);
    }
}

#[component]
pub fn ViewFilterPanel(filter: ViewFilter) -> impl IntoView {
    // Hold O (or the user's key) for the original.
    let keymap = prefs().keymap;
    let down = window_event_listener(ev::keydown, move |ev| {
        if !typing_in_a_field() && keymap.with_untracked(|keys| keys.matches(Action::Original, &press(&ev))) {
            filter.original.set(true);
            ev.prevent_default();
        }
    });
    let up = window_event_listener(ev::keyup, move |ev| {
        if keymap.with_untracked(|keys| keys.is_key(Action::Original, &ev.key())) {
            filter.original.set(false);
        }
    });
    // A key released in another window never comes back as a keyup.
    let blur = window_event_listener(ev::blur, move |_| filter.original.set(false));
    on_cleanup(move || {
        down.remove();
        up.remove();
        blur.remove();
    });

    let choice = move |preset: Option<Preset>, title: &'static str| {
        let on = move || filter.preset.get() == preset;
        view! {
            <button role="radio" aria-checked=move || on().to_string() class:active=on
                on:click={move |ev| {
                    filter.preset.set(preset);
                    // Back to the canvas, so the keys work.
                    if let Some(button) = ev.current_target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(t).ok()) {
                        button.blur().ok();
                    }
                }}>{title}</button>
        }
    };
    let sliders = move || {
        let preset = filter.preset.get()?;
        Some(
            preset
                .parameters()
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    let step = 10f64.powi(-(parameter.decimals as i32));
                    let decimals = parameter.decimals as usize;
                    let value = move || filter.value(preset, index);
                    view! {
                        <Slider label=parameter.label help=parameter.help range=(parameter.min, parameter.max) step=step
                            value=Signal::derive(value) shown=Signal::derive(move || format!("{:.decimals$}", value()))
                            on_input=move |v| filter.set_value(preset, index, v) />
                    }
                })
                .collect_view(),
        )
    };

    view! {
        <section class="view-filter" aria-label="View filter" class:collapsed=move || filter.collapsed.get()
            // Clicks and wheel turns here are not for the canvas below.
            on:pointerdown=|ev| ev.stop_propagation() on:wheel=|ev| ev.stop_propagation()>
            <div class="view-filter-head">
                <button class="collapse" aria-expanded=move || (!filter.collapsed.get()).to_string()
                    on:click=move |_| filter.collapsed.update(|c| *c = !*c)>
                    <span aria-hidden="true">{move || if filter.collapsed.get() { "▸" } else { "▾" }}</span>
                    "View filter"
                </button>
                <span class="muted">{move || if filter.loading.get() { "Filtering…" } else { "" }}</span>
                <span class="muted hold">"hold "<kbd>"O"</kbd>" = original"</span>
            </div>
            <Show when=move || !filter.collapsed.get()>
                <div class="presets" role="radiogroup" aria-label="Preset">
                    {choice(None, "Original")}
                    {Preset::ALL.into_iter().map(|preset| choice(Some(preset), preset.title())).collect_view()}
                </div>
                {sliders}
                <Slider label="Brightness" help="Applied on screen only" range=(0.0, 300.0) step=1.0
                    value=Signal::derive(move || filter.brightness.get()) shown=Signal::derive(move || format!("{:.0}%", filter.brightness.get()))
                    on_input=move |v| filter.brightness.set(v) />
                <Slider label="Contrast" help="Applied on screen only" range=(0.0, 300.0) step=1.0
                    value=Signal::derive(move || filter.contrast.get()) shown=Signal::derive(move || format!("{:.0}%", filter.contrast.get()))
                    on_input=move |v| filter.contrast.set(v) />
                <button class="reset small" on:click=move |_| filter.reset()>"Reset"</button>
            </Show>
        </section>
    }
}

/// A labelled range input with its value.
#[component]
fn Slider(
    label: &'static str,
    help: &'static str,
    range: (f64, f64),
    step: f64,
    value: Signal<f64>,
    shown: Signal<String>,
    on_input: impl Fn(f64) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <label class="slider" title=help>
            <span>{label}</span>
            <input type="range" min=range.0 max=range.1 step=step prop:value=move || value.get().to_string()
                on:input:target={move |ev| {
                    if let Ok(v) = ev.target().value().parse::<f64>() {
                        on_input(v);
                    }
                }}
                // Back to the canvas, so the keys work.
                on:change:target=move |ev| { ev.target().blur().ok(); } />
            <span class="mono value">{move || shown.get()}</span>
        </label>
    }
}
