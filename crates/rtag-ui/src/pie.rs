//! Blender-style pie menus: rounded buttons at compass positions around a small ring that opens
//! at the cursor. The ring's arc follows the pointer direction; what the direction picks comes
//! from `rtag_core::pie`.

use leptos::{ev, prelude::*};
use rtag_core::{
    geometry::Point,
    pie::{Compass, DEAD_ZONE, pick},
};

use crate::annotate::Tool;

const BUTTON_WIDTH: f64 = 118.0;
const WIDE_BUTTON_WIDTH: f64 = 150.0;
const BUTTON_HEIGHT: f64 = 32.0;
const RING_RADIUS: f64 = 24.0;
/// The direction arc covers this many degrees of the ring.
const ARC_DEGREES: f64 = 70.0;

#[derive(Debug, Clone, PartialEq)]
pub enum PieAction {
    Tool(Tool),
    Label(i64),
    /// Opens the searchable list of all labels.
    MoreLabels,
    ToggleSquare,
    ToggleFromCentre,
    /// Rectangle: three points for an oriented rectangle (true) or two for a box (false).
    SetOriented(bool),
    /// Lines: always snap to 15°.
    ToggleSnap,
    /// Polyline: drag to draw.
    ToggleFreehand,
    /// Points: one point per click.
    ToggleSinglePoint,
    /// Circle: two points are a diameter (true) or centre and radius (false).
    SetDiameter(bool),
    /// Brush: erase (true) or paint (false).
    SetEraser(bool),
    /// Brush: fuzzy fill (true) or painting (false).
    SetFill(bool),
    /// Freehand polylines: follow edges.
    ToggleEdgeSnap,
    /// Brush: paint only the region similar to the stroke's start.
    ToggleStopAtEdges,
    /// Fuzzy fill: a higher (true) or lower (false) threshold.
    Threshold(bool),
    /// Brush: a larger (true) or smaller (false) size.
    BrushSize(bool),
    /// Finish the polygon, polyline or points being drawn.
    FinishShape,
    /// Remove the last placed vertex.
    UndoPoint,
    /// Picks nothing new (the option already in use).
    Keep,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PieItem {
    pub slot: Compass,
    pub name: String,
    /// The key that picks this item while the pie is open (shown on the button).
    pub key: Option<String>,
    pub swatch: Option<String>,
    /// Disabled items keep their place so positions never shift.
    pub enabled: bool,
    pub current: bool,
    pub action: PieAction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pie {
    pub label: &'static str,
    pub items: Vec<PieItem>,
    /// Where it opened, in client (viewport) pixels.
    pub centre: Point,
    /// Four wider buttons (options pies) instead of eight.
    pub wide: bool,
    /// True while the opening key or button is still held: releasing then picks.
    pub held: bool,
    pub opened_at: f64,
}

impl Pie {
    /// The enabled item the pointer points at.
    pub fn highlighted(&self, pointer: Point) -> Option<&PieItem> {
        let slots: Vec<Compass> = self
            .items
            .iter()
            .filter(|item| item.enabled)
            .map(|item| item.slot)
            .collect();
        let slot = pick(pointer.x - self.centre.x, pointer.y - self.centre.y, &slots)?;
        self.items.iter().find(|item| item.slot == slot)
    }

    pub fn item_for_key(&self, key: &str) -> Option<&PieItem> {
        self.items.iter().find(|item| {
            item.enabled
                && item
                    .key
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case(key))
        })
    }
}

pub fn now() -> f64 {
    web_sys::js_sys::Date::now()
}

/// Shows `pie` when set. Clicking a button picks it; clicking anywhere else calls
/// `on_background`, which picks by direction as in Blender.
#[component]
pub fn PieMenu(
    pie: RwSignal<Option<Pie>>,
    pointer: RwSignal<Point>,
    on_pick: impl Fn(PieAction) + Copy + Send + Sync + 'static,
    on_background: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    move || {
        let current = pie.get()?;
        let centre = current.centre;
        let width = if current.wide {
            WIDE_BUTTON_WIDTH
        } else {
            BUTTON_WIDTH
        };
        let highlighted = move || {
            pie.with(|pie| {
                pie.as_ref()
                    .and_then(|pie| pie.highlighted(pointer.get()).map(|item| item.slot))
            })
        };
        // The arc points from the centre towards the pointer, once outside the dead zone.
        let arc = move || {
            let p = pointer.get();
            let (dx, dy) = (p.x - centre.x, p.y - centre.y);
            (dx.hypot(dy) >= DEAD_ZONE).then(|| dy.atan2(dx).to_degrees() - ARC_DEGREES / 2.0)
        };
        let circumference = std::f64::consts::TAU * RING_RADIUS;
        let arc_length = circumference * ARC_DEGREES / 360.0;
        let buttons = current
            .items
            .into_iter()
            .map(|item| {
                let at = item.slot.offset(current.wide);
                let slot = item.slot;
                let action = item.action.clone();
                let enabled = item.enabled;
                view! {
                    <button
                        role="menuitem"
                        class="pie-button"
                        class:hot=move || highlighted() == Some(slot)
                        class:current=item.current
                        disabled=!item.enabled
                        style:left=format!("{}px", at.x - width / 2.0)
                        style:top=format!("{}px", at.y - BUTTON_HEIGHT / 2.0)
                        style:width=format!("{width}px")
                        on:pointerdown=|ev: ev::PointerEvent| ev.stop_propagation()
                        // Clicks on a disabled button's text still arrive in some browsers.
                        on:click=move |_| if enabled { on_pick(action.clone()) }
                    >
                        {item.swatch.map(|color| view! { <span class="swatch" style:background=color></span> })}
                        <span class="grow">{item.name}</span>
                        {item.key.map(|key| view! { <span class="pie-key">{key}</span> })}
                    </button>
                }
            })
            .collect_view();
        Some(view! {
            <div class="pie-layer" on:pointerdown=move |ev: ev::PointerEvent| if ev.button() == 0 { on_background() }
                on:contextmenu=|ev: ev::MouseEvent| ev.prevent_default()>
                <div class="pie" role="menu" aria-label=current.label
                    style:left=format!("{}px", centre.x) style:top=format!("{}px", centre.y)>
                    <svg class="pie-ring" width="64" height="64" viewBox="0 0 64 64" aria-hidden="true">
                        <circle cx="32" cy="32" r=RING_RADIUS class="track"></circle>
                        {move || arc().map(|start| view! {
                            <circle cx="32" cy="32" r=RING_RADIUS class="arc"
                                stroke-dasharray=format!("{arc_length} {circumference}")
                                transform=format!("rotate({start} 32 32)")></circle>
                        })}
                    </svg>
                    {buttons}
                </div>
            </div>
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchEntry {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub key: Option<String>,
    pub enabled: bool,
}

/// "More…": every label in a list filtered by typing. Enter picks the first match.
#[component]
pub fn LabelSearch(
    at: Point,
    entries: Vec<SearchEntry>,
    on_pick: impl Fn(i64) + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let input = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(input) = input.get() {
            input.focus().ok();
        }
    });
    let matches = {
        let entries = entries.clone();
        move || {
            let needle = query.get().to_lowercase();
            entries
                .iter()
                .filter(|entry| entry.name.to_lowercase().contains(&needle))
                .cloned()
                .collect::<Vec<_>>()
        }
    };
    let first = {
        let matches = matches.clone();
        move || {
            matches()
                .into_iter()
                .find(|entry| entry.enabled)
                .map(|entry| entry.id)
        }
    };
    view! {
        <div class="pie-layer" on:pointerdown=move |_| on_close()>
            <div class="label-search" role="dialog" aria-label="All labels"
                style:left=format!("{}px", at.x) style:top=format!("{}px", at.y)
                on:pointerdown=|ev: ev::PointerEvent| ev.stop_propagation()>
                <input node_ref=input class="search" placeholder="Type to find a label" aria-label="Find a label"
                    prop:value=query
                    on:input:target=move |ev| query.set(ev.target().value())
                    on:keydown={move |ev: ev::KeyboardEvent| match ev.key().as_str() {
                        "Enter" => if let Some(id) = first() { on_pick(id) },
                        "Escape" => on_close(),
                        _ => {}
                    }} />
                <ul>
                    {move || matches().into_iter().map(|entry| {
                        let id = entry.id;
                        view! {
                            <li class:disabled=!entry.enabled
                                on:click=move |_| if entry.enabled { on_pick(id) }>
                                <span class="swatch" style:background=entry.color.clone()></span>
                                <span class="grow">{entry.name.clone()}</span>
                                {entry.key.clone().map(|key| view! { <kbd>{key}</kbd> })}
                            </li>
                        }
                    }).collect_view()}
                </ul>
            </div>
        </div>
    }
}
