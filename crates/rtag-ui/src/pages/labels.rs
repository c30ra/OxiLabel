use leptos::{prelude::*, task::spawn_local};
use rtag_core::{
    Label, LabelDefinition, ShapeType,
    annotation::LabelUsage,
    attribute::{AttributeDefinition, AttributeKind},
};

use crate::api;

const DEFAULT_COLOR: &str = "#e74c3c";

/// The label set of one project: a table of labels and a form to add or edit one.
#[component]
pub fn LabelSet(project: i64) -> impl IntoView {
    let url = format!("/api/projects/{project}/labels");
    let labels = LocalResource::new({
        let url = url.clone();
        move || {
            let url = url.clone();
            async move { api::get::<Vec<Label>>(&url).await }
        }
    });

    // Form state. `editing` holds the id of the label being edited, or none when adding.
    let editing = RwSignal::new(None::<i64>);
    let name = RwSignal::new(String::new());
    let color = RwSignal::new(DEFAULT_COLOR.to_owned());
    let hotkey = RwSignal::new(String::new());
    let shapes = RwSignal::new(ShapeType::ALL.to_vec());
    let ignore = RwSignal::new(false);
    let attributes: AttributeRows = RwSignal::new(Vec::new());
    let error = RwSignal::new(None::<String>);

    let reset = move || {
        editing.set(None);
        name.set(String::new());
        color.set(DEFAULT_COLOR.to_owned());
        hotkey.set(String::new());
        shapes.set(ShapeType::ALL.to_vec());
        ignore.set(false);
        attributes.set(Vec::new());
        error.set(None);
    };

    let start_edit = move |label: Label| {
        editing.set(Some(label.id));
        name.set(label.name);
        color.set(label.color);
        hotkey.set(label.hotkey.unwrap_or_default());
        shapes.set(label.shape_types);
        ignore.set(label.ignore);
        attributes.set(attribute_rows(label.attributes));
        error.set(None);
    };

    let on_submit = {
        let url = url.clone();
        move |ev: leptos::ev::SubmitEvent| {
            ev.prevent_default();
            let definition = LabelDefinition {
                name: name.get(),
                color: color.get(),
                hotkey: Some(hotkey.get()),
                shape_types: ShapeType::ALL
                    .into_iter()
                    .filter(|shape| shapes.read().contains(shape))
                    .collect(),
                ignore: ignore.get(),
                attributes: attributes.get().iter().map(|(_, row)| row.get()).collect(),
            };
            let url = url.clone();
            spawn_local(async move {
                let saved = match editing.get_untracked() {
                    Some(id) => api::put::<_, Label>(&format!("{url}/{id}"), &definition).await,
                    None => api::post::<_, Label>(&url, &definition).await,
                };
                match saved {
                    Ok(_) => {
                        reset();
                        labels.refetch();
                    }
                    Err(message) => error.set(Some(message)),
                }
            });
        }
    };

    let on_delete = {
        let url = url.clone();
        move |label: Label| {
            let url = format!("{url}/{}", label.id);
            spawn_local(async move {
                // Say how many annotations go with the label before deleting it.
                let used = match api::get::<LabelUsage>(&format!("{url}/usage")).await {
                    Ok(usage) => usage.annotations,
                    Err(message) => return error.set(Some(message)),
                };
                let question = match used {
                    0 => format!("Delete label \"{}\"?", label.name),
                    1 => format!(
                        "Delete label \"{}\" and the 1 annotation that uses it? This cannot be undone.",
                        label.name
                    ),
                    n => format!(
                        "Delete label \"{}\" and the {n} annotations that use it? This cannot be undone.",
                        label.name
                    ),
                };
                if !window().confirm_with_message(&question).unwrap_or(false) {
                    return;
                }
                let url = if used > 0 {
                    format!("{url}?confirm=true")
                } else {
                    url
                };
                match api::delete(&url).await {
                    Ok(()) => {
                        if editing.get_untracked() == Some(label.id) {
                            reset();
                        }
                        labels.refetch();
                    }
                    Err(message) => error.set(Some(message)),
                }
            });
        }
    };

    view! {
        {move || match labels.get() {
            None => view! { <p>"Loading…"</p> }.into_any(),
            Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
            Some(Ok(list)) if list.is_empty() => view! { <p>"No labels yet."</p> }.into_any(),
            Some(Ok(list)) => {
                let on_delete = on_delete.clone();
                view! {
                    <table class="labels">
                        <thead>
                            <tr><th>"Id"</th><th>"Name"</th><th>"Key"</th><th>"Shapes"</th><th></th></tr>
                        </thead>
                        <tbody>
                            {list.into_iter().map(|label| {
                                let to_edit = label.clone();
                                let to_delete = label.clone();
                                let on_delete = on_delete.clone();
                                let shapes = label.shape_types.iter().map(|shape| shape.name()).collect::<Vec<_>>().join(", ");
                                view! {
                                    <tr>
                                        <td>{label.id}</td>
                                        <td>
                                            <span class="swatch" style:background=label.color.clone()></span>
                                            {label.name.clone()}
                                            {label.ignore.then(|| view! { <span class="badge">"ignore"</span> })}
                                        </td>
                                        <td><kbd>{label.hotkey.clone()}</kbd></td>
                                        <td class="shapes">{shapes}</td>
                                        <td class="actions">
                                            <button on:click=move |_| start_edit(to_edit.clone())>"Edit"</button>
                                            <button on:click=move |_| on_delete(to_delete.clone())>"Delete"</button>
                                        </td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                }.into_any()
            }
        }}

        <form class="label-form" on:submit=on_submit>
            <h3>{move || if editing.get().is_some() { "Edit label" } else { "Add label" }}</h3>
            <div class="row">
                <input placeholder="Name" prop:value=name on:input:target=move |ev| name.set(ev.target().value()) />
                <input type="color" title="Color" prop:value=color on:input:target=move |ev| color.set(ev.target().value()) />
                <input class="key" placeholder="Key" maxlength="1" title="Hotkey"
                    prop:value=hotkey on:input:target=move |ev| hotkey.set(ev.target().value()) />
            </div>
            <fieldset>
                <legend>"Allowed shapes"</legend>
                {ShapeType::ALL.into_iter().map(|shape| view! {
                    <label class="check">
                        <input type="checkbox"
                            prop:checked=move || shapes.read().contains(&shape)
                            on:change:target=move |ev| {
                                let checked = ev.target().checked();
                                shapes.update(|list| {
                                    list.retain(|s| *s != shape);
                                    if checked {
                                        list.push(shape);
                                    }
                                });
                            } />
                        {shape.name()}
                    </label>
                }).collect_view()}
            </fieldset>
            <AttributeList rows=attributes />
            <label class="check">
                <input type="checkbox" prop:checked=ignore on:change:target=move |ev| ignore.set(ev.target().checked()) />
                "Ignore label (uncertain regions, skipped in training)"
            </label>
            <Show when=move || error.get().is_some()>
                <p class="error" role="alert">{move || error.get()}</p>
            </Show>
            <div class="row">
                <button class="primary" type="submit">{move || if editing.get().is_some() { "Save" } else { "Add label" }}</button>
                <Show when=move || editing.get().is_some()>
                    <button type="button" on:click=move |_| reset()>"Cancel"</button>
                </Show>
            </div>
        </form>
    }
}

/// The rows of the attribute list: a stable id (for the keyed list) and the definition, each in
/// its own signal so typing in one field does not rebuild the others.
type AttributeRows = RwSignal<Vec<(u64, RwSignal<AttributeDefinition>)>>;

fn attribute_rows(definitions: Vec<AttributeDefinition>) -> Vec<(u64, RwSignal<AttributeDefinition>)> {
    static NEXT_ROW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    definitions
        .into_iter()
        .map(|definition| (NEXT_ROW.fetch_add(1, std::sync::atomic::Ordering::Relaxed), RwSignal::new(definition)))
        .collect()
}

/// The attributes of the label being edited: a row per attribute with its name, type and the
/// type's settings (options, range, default).
#[component]
fn AttributeList(rows: AttributeRows) -> impl IntoView {
    let number = |text: String| text.trim().parse::<f64>().ok();
    let settings = move |row: RwSignal<AttributeDefinition>, kind: &'static str| match kind {
        "checkbox" => view! {
            <label class="check">
                <input class="default" type="checkbox"
                    prop:checked=move || row.with(|a| matches!(a.kind, AttributeKind::Checkbox { default: true }))
                    on:change:target=move |ev| row.update(|a| a.kind = AttributeKind::Checkbox { default: ev.target().checked() }) />
                "on by default"
            </label>
        }.into_any(),
        "select" => {
            let options = move || row.with(|a| match &a.kind {
                AttributeKind::Select { options, default } => (options.clone(), default.clone()),
                _ => (Vec::new(), String::new()),
            });
            view! {
                <input class="options" placeholder="Options, comma separated" prop:value=untrack(options).0.join(", ")
                    on:input:target=move |ev| {
                        let options: Vec<String> = ev.target().value().split(',').map(|o| o.trim().to_owned()).filter(|o| !o.is_empty()).collect();
                        row.update(|a| {
                            let AttributeKind::Select { default, .. } = &a.kind else { return };
                            let default = if options.contains(default) { default.clone() } else { options.first().cloned().unwrap_or_default() };
                            a.kind = AttributeKind::Select { options, default };
                        });
                    } />
                <select class="default" title="Default" on:change:target=move |ev| row.update(|a| {
                    if let AttributeKind::Select { default, .. } = &mut a.kind {
                        *default = ev.target().value();
                    }
                })>
                    {move || {
                        let (choices, default) = options();
                        choices.into_iter().map(|option| {
                            let selected = option == default;
                            view! { <option value=option.clone() selected=selected>{option.clone()}</option> }
                        }).collect_view()
                    }}
                </select>
            }.into_any()
        }
        "number" => {
            let (min, max, default) = row.with_untracked(|a| match a.kind {
                AttributeKind::Number { min, max, default } => (min, max, default),
                _ => (None, None, 0.0),
            });
            let shown = |value: Option<f64>| value.map(|v| v.to_string()).unwrap_or_default();
            view! {
                <input class="min" type="number" step="any" placeholder="min" prop:value=shown(min)
                    on:input:target=move |ev| {
                        let value = number(ev.target().value());
                        row.update(|a| if let AttributeKind::Number { min, .. } = &mut a.kind { *min = value });
                    } />
                <input class="max" type="number" step="any" placeholder="max" prop:value=shown(max)
                    on:input:target=move |ev| {
                        let value = number(ev.target().value());
                        row.update(|a| if let AttributeKind::Number { max, .. } = &mut a.kind { *max = value });
                    } />
                <input class="default" type="number" step="any" title="Default" prop:value=default.to_string()
                    on:input:target=move |ev| {
                        // A half-typed number (e.g. "-") keeps the last default.
                        if let Some(value) = number(ev.target().value()) {
                            row.update(|a| if let AttributeKind::Number { default, .. } = &mut a.kind { *default = value });
                        }
                    } />
            }.into_any()
        }
        _ => {
            let default = row.with_untracked(|a| match &a.kind {
                AttributeKind::Text { default } => default.clone(),
                _ => String::new(),
            });
            view! {
                <input class="default" placeholder="Default text" prop:value=default
                    on:input:target=move |ev| row.update(|a| a.kind = AttributeKind::Text { default: ev.target().value() }) />
            }.into_any()
        }
    };
    let row_view = move |(id, row): (u64, RwSignal<AttributeDefinition>)| {
        // The settings are rebuilt only when the type changes, not on every keystroke.
        let kind = Memo::new(move |_| row.with(|a| a.kind.type_name()));
        view! {
            <div class="attribute">
                <input class="attribute-name" placeholder="Attribute name" prop:value=row.with_untracked(|a| a.name.clone())
                    on:input:target=move |ev| row.update(|a| a.name = ev.target().value()) />
                <select class="attribute-type" title="Type" on:change:target=move |ev| {
                    if let Some(blank) = AttributeKind::blank(&ev.target().value()) {
                        row.update(|a| a.kind = blank);
                    }
                }>
                    {AttributeKind::TYPES.into_iter().map(|value| view! {
                        <option value=value selected=move || kind.get() == value>{capitalised(value)}</option>
                    }).collect_view()}
                </select>
                {move || settings(row, kind.get())}
                <button type="button" class="remove" title="Remove this attribute" aria-label="Remove attribute"
                    on:click=move |_| rows.update(|list| list.retain(|(row_id, _)| *row_id != id))>"×"</button>
            </div>
        }
    };
    view! {
        <fieldset class="attributes">
            <legend>"Attributes"</legend>
            <div class="attribute-rows">
                <For each=move || rows.get() key=|(id, _)| *id children=row_view />
            </div>
            <button type="button" on:click=move |_| rows.update(|list| {
                list.extend(attribute_rows(vec![AttributeDefinition { name: String::new(), kind: AttributeKind::Text { default: String::new() } }]))
            })>"Add attribute"</button>
        </fieldset>
    }
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
