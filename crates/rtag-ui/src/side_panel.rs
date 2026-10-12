//! The panel right of the image: the image's tags, and the attribute editor of the selected
//! annotation or tag. It is shown only when there is something in it: tags, or a selection whose
//! label has attributes.

use std::collections::BTreeSet;

use leptos::prelude::*;
use rtag_core::{
    Label,
    attribute::{AttributeDefinition, AttributeKind, AttributeValue},
};

use crate::annotate::Editor;

#[component]
pub fn SidePanel(editor: Editor, labels: Vec<Label>) -> impl IntoView {
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

    view! {
        <Show when=move || !tags().is_empty() || editing.with(Option::is_some)>
            <aside class="side-panel" aria-label="Tags and attributes">
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
            </aside>
        </Show>
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
