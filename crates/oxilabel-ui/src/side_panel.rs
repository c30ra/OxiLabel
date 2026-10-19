//! The panel right of the image: the collapsible objects list (select, hide, lock, relabel,
//! accept a pre-annotation), the review of pre-annotations (confidence threshold, accept or delete
//! in bulk), the image's tags, the attribute editor of the selected annotation or tag (shown when
//! its label has attributes), and whatever the workspace adds below (running a model).

use std::collections::BTreeSet;

use leptos::prelude::*;
use oxilabel_core::{
    Label,
    annotation::{Annotation, Shape},
    attribute::{AttributeDefinition, AttributeKind, AttributeValue},
    ellipse::Ellipse,
    history::Edit,
};

use crate::annotate::Editor;

#[component]
pub fn SidePanel(editor: Editor, labels: Vec<Label>, children: Children) -> impl IntoView {
    let labels = StoredValue::new(labels);
    let label = move |id: i64| labels.with_value(|labels| labels.iter().find(|l| l.id == id).cloned());
    let tags = move || {
        editor.annotations.with(|list| {
            list.iter().enumerate().filter(|(_, a)| a.shape.is_tag()).map(|(index, a)| (index, a.label)).collect::<Vec<_>>()
        })
    };
    // The single selected annotation and its label, if that label has attributes. A memo, so the
    // editor is rebuilt only when the selection or its label changes, not on every edit.
    let editing = Memo::new(move |_| {
        let index = editor.selection.with(|set| (set.len() == 1).then(|| set.first().copied()).flatten())?;
        let id = editor.annotations.with(|list| list.get(index).map(|a| a.label))?;
        label(id).filter(|label| !label.attributes.is_empty()).map(|label| (index, label))
    });

    let collapsed = RwSignal::new(false);
    // Every annotation but the tags, in drawing order (the topmost last).
    let objects = move || {
        editor.annotations.with(|list| list.iter().enumerate().filter(|(_, a)| !a.shape.is_tag()).map(|(i, a)| (i, a.clone())).collect::<Vec<_>>())
    };
    let relabel = move |index: usize, id: i64| {
        let Some(before) = editor.annotations.with_untracked(|list| list.get(index).cloned()) else { return };
        let Some(target) = label(id) else { return };
        // Relabelling a pre-annotation reviews it.
        let after = Annotation { label: id, attributes: target.conform_attributes(&before.attributes), pending: false, ..before.clone() };
        if after != before {
            editor.apply(Edit::Replace { index, before, after });
        }
    };
    let row = move |index: usize, annotation: Annotation| {
        let color = label(annotation.label).map(|l| l.color).unwrap_or_default();
        let shape = annotation.shape.shape_type();
        let choices = labels.with_value(|labels| labels.iter().filter(|l| l.shape_types.contains(&shape)).cloned().collect::<Vec<_>>());
        let name = label(annotation.label).map(|l| l.name).unwrap_or_default();
        let selected = move || editor.selection.with(|set| set.contains(&index));
        let (hidden, locked, pending) = (annotation.hidden, annotation.locked, annotation.pending);
        view! {
            // Not "hidden"/"locked": the global .hidden class hides elements.
            <li class:selected=selected class:is-hidden=hidden class:is-locked=locked class:pending=pending>
                <span class="swatch" style:background=color></span>
                <select class="relabel" aria-label="Label" on:change:target=move |ev| {
                    if let Ok(id) = ev.target().value().parse() { relabel(index, id) }
                }>
                    {choices.into_iter().map(|l| {
                        let current = l.id == annotation.label;
                        view! { <option value=l.id.to_string() selected=current>{l.name}</option> }
                    }).collect_view()}
                </select>
                <button class="pick" title=if hidden { "Hidden: show it to select it" } else { "Select (Shift adds)" } on:click=move |ev| {
                    if hidden {
                        return;
                    }
                    if ev.shift_key() {
                        editor.selection.update(|set| if !set.remove(&index) { set.insert(index); });
                    } else {
                        editor.selection.set(BTreeSet::from([index]));
                    }
                }>
                    <span class="detail">{describe(&annotation)}</span>
                </button>
                {pending.then(|| view! {
                    <button class="accept" title="Accept this pre-annotation" aria-label=format!("Accept {name}")
                        on:click=move |_| editor.accept(&[index])>"✓"</button>
                })}
                <button class="hide" aria-pressed=hidden.to_string() title=if hidden { "Show" } else { "Hide" }
                    aria-label=format!("{} {name}", if hidden { "Show" } else { "Hide" })
                    on:click=move |_| editor.set_flag(index, Some(!hidden), None)>
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d=if hidden { EYE_OFF } else { EYE } /></svg>
                </button>
                <button class="lock" aria-pressed=locked.to_string() title=if locked { "Unlock" } else { "Lock" }
                    aria-label=format!("{} {name}", if locked { "Unlock" } else { "Lock" })
                    on:click=move |_| editor.set_flag(index, None, Some(!locked))>
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d=if locked { LOCKED } else { UNLOCKED } /></svg>
                </button>
            </li>
        }
    };

    view! {
        <aside class="side-panel" aria-label="Objects, tags and attributes">
                <section class="objects" class:collapsed=move || collapsed.get()>
                    <h4>
                        <button class="collapse" aria-expanded=move || (!collapsed.get()).to_string()
                            on:click=move |_| collapsed.update(|c| *c = !*c)>
                            <span aria-hidden="true">{move || if collapsed.get() { "▸" } else { "▾" }}</span>
                            "Objects"
                            <span class="muted mono">{move || objects().len()}</span>
                        </button>
                    </h4>
                    <Show when=move || !collapsed.get()>
                        <ul>
                            {move || objects().into_iter().map(|(index, annotation)| row(index, annotation)).collect_view()}
                        </ul>
                        <Show when=move || objects().is_empty()>
                            <p class="muted empty">"Nothing drawn yet."</p>
                        </Show>
                    </Show>
                </section>
                <Show when=move || editor.annotations.with(|list| list.iter().any(|a| a.pending))>
                    <section class="review" aria-label="Pre-annotations to review">
                        <h4>
                            "To review"
                            <span class="muted mono">{move || { let (shown, below) = editor.pending(); shown.len() + below.len() }}</span>
                        </h4>
                        <label class="threshold" title="Pre-annotations less confident than this are hidden">
                            "Confidence from"
                            <input type="range" min="0" max="1" step="0.05" prop:value=move || editor.pre_annotation_threshold.get().to_string()
                                on:input:target={move |ev| if let Ok(value) = ev.target().value().parse::<f64>() {
                                    editor.pre_annotation_threshold.set(value);
                                    // What the threshold hides cannot stay selected.
                                    let hidden: Vec<usize> = editor.annotations.with_untracked(|list| {
                                        list.iter().enumerate().filter(|(_, a)| !a.shown(value)).map(|(index, _)| index).collect()
                                    });
                                    editor.selection.update(|set| set.retain(|index| !hidden.contains(index)));
                                }} />
                            <span class="mono">{move || format!("{:.2}", editor.pre_annotation_threshold.get())}</span>
                        </label>
                        <div class="review-actions">
                            <button class="accept-shown" disabled=move || editor.pending().0.is_empty()
                                on:click=move |_| editor.accept(&editor.pending().0)>{move || format!("Accept {}", editor.pending().0.len())}</button>
                            <button class="danger delete-below" disabled=move || editor.pending().1.is_empty()
                                on:click=move |_| editor.remove_all(&editor.pending().1)>{move || format!("Delete {} below", editor.pending().1.len())}</button>
                        </div>
                    </section>
                </Show>
                <Show when=move || !tags().is_empty()>
                    <section class="tags">
                        <h4>"Tags"</h4>
                        <ul>
                            {move || tags().into_iter().filter_map(|(index, id)| {
                                let label = label(id)?;
                                let selected = move || editor.selection.with(|set| set.contains(&index));
                                Some(view! {
                                    <li class:selected=selected>
                                        <span class="swatch" style:background=label.color.clone()></span>
                                        <button class="name" title="Select this tag"
                                            on:click=move |_| editor.selection.set(BTreeSet::from([index]))>{label.name.clone()}</button>
                                        <button class="remove" title="Remove this tag" aria-label=format!("Remove tag {}", label.name)
                                            on:click=move |_| editor.remove(index)>"×"</button>
                                    </li>
                                })
                            }).collect_view()}
                        </ul>
                    </section>
                </Show>
                {move || editing.get().map(|(index, label)| view! { <AttributeEditor editor=editor index=index label=label /> })}
                {children()}
        </aside>
    }
}

// Stroke icons (paths in a 24 × 24 box), as in the approved workspace design.
const EYE: &str = "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0";
const EYE_OFF: &str = "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z M4 4l16 16";
const LOCKED: &str = "M5 11h14v10H5z M8 11V8a4 4 0 0 1 8 0v3";
const UNLOCKED: &str = "M5 11h14v10H5z M8 11V8a4 4 0 0 1 7.5-2";

/// What an object is, briefly: its shape with size or point count, and where it came from.
fn describe(annotation: &Annotation) -> String {
    let count = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let shape = match &annotation.shape {
        Shape::Rectangle(rect) => format!("rectangle {:.0} × {:.0}", rect.width(), rect.height()),
        Shape::OrientedRectangle(rect) => format!("oriented rectangle {:.0} × {:.0}", rect.width, rect.height),
        Shape::Polygon { points } => format!("polygon · {}", count(points.len(), "point")),
        Shape::Polyline { points } => format!("polyline · {}", count(points.len(), "point")),
        Shape::Points { points } => format!("points · {}", points.len()),
        Shape::Ellipse(ellipse) => format!("{} {}", if ellipse.is_circle() { "circle" } else { "ellipse" }, ellipse_size(ellipse)),
        Shape::Mask(mask) => format!("mask {} × {}", mask.width, mask.height),
        Shape::Tag => "tag".to_owned(),
    };
    match (&annotation.source, annotation.confidence) {
        (Some(source), Some(confidence)) => format!("{shape} · {source} {confidence:.2}"),
        (Some(source), None) => format!("{shape} · {source}"),
        _ => shape,
    }
}

/// The attribute values of annotation `index`, one field per attribute of its label. The fields
/// follow the stored values (undo, redo); each change is one undo step.
#[component]
fn AttributeEditor(editor: Editor, index: usize, label: Label) -> impl IntoView {
    let target = StoredValue::new(label.clone());
    // The current value of attribute `name`, its default if unset.
    let value = move |name: &str| {
        editor.annotations.with(|list| {
            let annotation = list.get(index)?;
            target.with_value(|label| label.conform_attributes(&annotation.attributes)).remove(name)
        })
    };
    let set = move |name: String, value: AttributeValue| {
        editor.change_selected(|annotation| {
            let mut changed = annotation.clone();
            changed.attributes = target.with_value(|label| label.conform_attributes(&annotation.attributes));
            changed.attributes.insert(name.clone(), value.clone());
            changed
        });
    };
    let text_of = |value: Option<AttributeValue>| match value {
        Some(AttributeValue::Text(text)) => text,
        Some(AttributeValue::Number(number)) => number.to_string(),
        _ => String::new(),
    };
    let field = move |definition: AttributeDefinition| {
        let key = StoredValue::new(definition.name.clone());
        let current = move || value(&key.get_value());
        match definition.kind {
            AttributeKind::Checkbox { .. } => view! {
                <label class="check">
                    <input type="checkbox" prop:checked=move || matches!(current(), Some(AttributeValue::Bool(true)))
                        on:change:target=move |ev| set(key.get_value(), AttributeValue::Bool(ev.target().checked())) />
                    {definition.name}
                </label>
            }
            .into_any(),
            AttributeKind::Select { options, .. } => view! {
                <label>
                    <span>{definition.name}</span>
                    <select on:change:target=move |ev| set(key.get_value(), AttributeValue::Text(ev.target().value()))>
                        {options.into_iter().map(|option| {
                            let mine = option.clone();
                            view! { <option value=option.clone() prop:selected=move || text_of(current()) == mine>{option.clone()}</option> }
                        }).collect_view()}
                    </select>
                </label>
            }
            .into_any(),
            AttributeKind::Number { min, max, .. } => {
                let check = StoredValue::new(AttributeDefinition { name: definition.name.clone(), kind: AttributeKind::Number { min, max, default: 0.0 } });
                view! {
                    <label>
                        <span>{definition.name}</span>
                        <input type="number" step="any" min=min max=max prop:value=move || text_of(current())
                            on:change:target=move |ev| {
                                let typed = ev.target().value().trim().parse::<f64>().ok().map(AttributeValue::Number);
                                match typed.filter(|value| check.with_value(|d| d.check(value).is_ok())) {
                                    Some(value) => set(key.get_value(), value),
                                    // Not taken: the field shows the stored value again.
                                    None => {
                                        ev.target().set_value(&text_of(current()));
                                        editor.notice.set(Some(format!("“{}” must be a number in its range.", key.get_value())));
                                    }
                                }
                                ev.target().blur().ok();
                            } />
                    </label>
                }
                .into_any()
            }
            AttributeKind::Text { .. } => view! {
                <label>
                    <span>{definition.name}</span>
                    <input prop:value=move || text_of(current()) on:change:target=move |ev| {
                        set(key.get_value(), AttributeValue::Text(ev.target().value()));
                        ev.target().blur().ok();
                    } />
                </label>
            }
            .into_any(),
        }
    };
    let kind = editor.annotations.with_untracked(|list| list.get(index).map(|a| a.shape.shape_type().name().to_lowercase())).unwrap_or_default();
    view! {
        <section class="attribute-editor" aria-label="Attributes of the selection">
            <h4>
                <span class="swatch" style:background=label.color.clone()></span>
                {label.name.clone()}
                <span class="muted">{format!(" · {kind}")}</span>
            </h4>
            {label.attributes.into_iter().map(field).collect_view()}
        </section>
    }
}

/// An ellipse's size: "r 12" for a circle, else its axes, e.g. "24 × 10".
pub fn ellipse_size(ellipse: &Ellipse) -> String {
    if ellipse.is_circle() { format!("r {:.0}", ellipse.rx) } else { format!("{:.0} × {:.0}", 2.0 * ellipse.rx, 2.0 * ellipse.ry) }
}
