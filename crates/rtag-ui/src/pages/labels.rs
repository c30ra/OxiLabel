use leptos::{prelude::*, task::spawn_local};
use rtag_core::{Label, LabelDefinition, ShapeType, annotation::LabelUsage};

use crate::api;

const DEFAULT_COLOR: &str = "#e74c3c";

fn shape_name(shape: ShapeType) -> &'static str {
    match shape {
        ShapeType::Rectangle => "Rectangle",
        ShapeType::OrientedRectangle => "Oriented rectangle",
        ShapeType::Polygon => "Polygon",
        ShapeType::Polyline => "Polyline",
        ShapeType::Points => "Points",
        ShapeType::Circle => "Circle",
        ShapeType::Mask => "Mask",
        ShapeType::Tag => "Tag",
    }
}

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
    let error = RwSignal::new(None::<String>);

    let reset = move || {
        editing.set(None);
        name.set(String::new());
        color.set(DEFAULT_COLOR.to_owned());
        hotkey.set(String::new());
        shapes.set(ShapeType::ALL.to_vec());
        ignore.set(false);
        error.set(None);
    };

    let start_edit = move |label: Label| {
        editing.set(Some(label.id));
        name.set(label.name);
        color.set(label.color);
        hotkey.set(label.hotkey.unwrap_or_default());
        shapes.set(label.shape_types);
        ignore.set(label.ignore);
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
                                let shapes = label.shape_types.iter().map(|shape| shape_name(*shape)).collect::<Vec<_>>().join(", ");
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
                        {shape_name(shape)}
                    </label>
                }).collect_view()}
            </fieldset>
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
