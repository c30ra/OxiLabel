//! Relabelling: one session rule for the label pie and the side panel. A label has each tag at
//! most once, attributes are made to fit the new label, a pre-annotation becomes reviewed, and
//! annotations that are not shown are left alone.

use std::collections::BTreeSet;

use super::*;
use crate::attribute::{AttributeDefinition, AttributeKind, Attributes};

fn attrs(value: serde_json::Value) -> Attributes {
    serde_json::from_value(value).expect("attribute values")
}

/// A label with a "severity" select (low by default, high allowed).
fn label_with_severity(id: i64) -> Label {
    let mut label = label_with(id, serde_json::json!({}));
    label.attributes = vec![AttributeDefinition {
        name: "severity".into(),
        kind: AttributeKind::Select { options: vec!["low".into(), "high".into()], default: "low".into() },
    }];
    label
}

fn tag(label: i64) -> Annotation {
    Annotation::manual(label, Shape::Tag)
}

fn tags(session: &Session) -> Vec<i64> {
    session.annotations().iter().filter(|a| a.shape.is_tag()).map(|a| a.label).collect()
}

fn select(session: &mut Session, indices: &[usize]) {
    session.set_selection(indices.iter().copied().collect::<BTreeSet<_>>());
}

#[test]
fn relabelling_conforms_attributes_and_reviews_a_pre_annotation() {
    let pre_annotation = Annotation {
        attributes: attrs(serde_json::json!({"severity": "high", "colour": "red"})),
        pending: true,
        confidence: Some(0.9),
        ..boxed(100.0, 100.0, 300.0, 300.0)
    };
    let mut session = Session::new(vec![pre_annotation], vec![label_with(1, serde_json::json!({})), label_with_severity(2)], IMAGE);
    select(&mut session, &[0]);

    session.relabel_selection(2);

    let annotation = &session.annotations()[0];
    assert_eq!(annotation.label, 2);
    assert_eq!(annotation.attributes, attrs(serde_json::json!({"severity": "high"})));
    assert!(!annotation.pending, "a relabelled pre-annotation is reviewed");
    assert_eq!(session.revision(), 1, "one undo step");
    session.undo();
    assert_eq!(session.annotations()[0].label, 1, "one undo restores the label and its attributes");
    assert!(session.annotations()[0].pending);
}

#[test]
fn a_tag_cannot_become_a_tag_the_image_already_has() {
    let mut session = Session::new(
        vec![tag(1), tag(2), boxed(100.0, 100.0, 200.0, 200.0)],
        vec![label_with(1, serde_json::json!({})), label_with(2, serde_json::json!({}))],
        IMAGE,
    );
    select(&mut session, &[0]);

    session.relabel_selection(2);

    assert_eq!(tags(&session), vec![1, 2], "the existing tag 2 stays and tag 1 does not join it");
    assert_eq!(session.annotations()[0].label, 1);
    assert_eq!(session.take_notice().as_deref(), Some("The image already has the tag “label 2”."));
    assert_eq!(session.revision(), 0, "nothing changed");
}

#[test]
fn relabelling_two_tags_to_one_label_leaves_one_tag() {
    let mut session = Session::new(
        vec![tag(1), tag(2)],
        vec![label_with(1, serde_json::json!({})), label_with(2, serde_json::json!({})), label_with(3, serde_json::json!({}))],
        IMAGE,
    );
    select(&mut session, &[0, 1]);

    session.relabel_selection(3);

    assert_eq!(tags(&session), vec![3, 2], "only the first selected tag takes the label");
}

#[test]
fn relabelling_leaves_hidden_annotations_alone() {
    let hidden = Annotation { hidden: true, ..boxed(100.0, 100.0, 200.0, 200.0) };
    let visible = boxed(300.0, 100.0, 400.0, 200.0);
    let mut session =
        Session::new(vec![hidden, visible], vec![label_with(1, serde_json::json!({})), label_with(2, serde_json::json!({}))], IMAGE);
    select(&mut session, &[0, 1]);

    session.relabel_selection(2);

    assert_eq!(session.annotations()[0].label, 1, "a hidden annotation is not relabelled");
    assert_eq!(session.annotations()[1].label, 2);
}

#[test]
fn the_side_panel_relabels_one_annotation_through_the_same_rule() {
    let hidden = Annotation { hidden: true, ..boxed(100.0, 100.0, 200.0, 200.0) };
    let mut session = Session::new(
        vec![tag(1), tag(2), hidden, boxed(300.0, 100.0, 400.0, 200.0)],
        vec![label_with(1, serde_json::json!({})), label_with(2, serde_json::json!({}))],
        IMAGE,
    );

    // A hidden row's label cannot be changed from the panel either.
    session.relabel(&[2], 2);
    assert_eq!(session.annotations()[2].label, 1);

    // A tag cannot take a label the image already has as a tag, nor can a second tag be added.
    session.relabel(&[0], 2);
    assert_eq!(tags(&session), vec![1, 2]);
    assert_eq!(session.revision(), 0);
    assert_eq!(session.take_notice().as_deref(), Some("The image already has the tag “label 2”."));

    session.relabel(&[3], 2);
    assert_eq!(session.annotations()[3].label, 2);
    assert_eq!(session.revision(), 1);
}