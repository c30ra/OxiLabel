//! Copying annotations from one image and pasting them on another of the same project, at the
//! same image coordinates. Copies are fresh manual work; tags are never copied.

use crate::{Label, annotation::Annotation};

/// What the clipboard keeps of `selected`: their shapes, labels and attribute values, as manual
/// work (no model source or confidence, not pending, locked or hidden). Tags are left out.
pub fn copied(selected: &[Annotation]) -> Vec<Annotation> {
    selected
        .iter()
        .filter(|annotation| !annotation.shape.is_tag())
        .map(|annotation| Annotation { attributes: annotation.attributes.clone(), ..Annotation::manual(annotation.label, annotation.shape.clone()) })
        .collect()
}

/// The copies pasted on an image, and how many were left out for each reason.
#[derive(Debug, Default, PartialEq)]
pub struct Pasted {
    pub annotations: Vec<Annotation>,
    pub too_big: usize,
    pub label_deleted: usize,
    pub not_allowed: usize,
}

impl Pasted {
    /// What the user is told about the copies left out, if any.
    pub fn notice(&self) -> Option<String> {
        let reasons: Vec<(usize, &str)> = [
            (self.too_big, "too big for this image"),
            (self.label_deleted, "label deleted"),
            (self.not_allowed, "shape type not allowed by its label"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .collect();
        let total: usize = reasons.iter().map(|(count, _)| count).sum();
        let shapes = if total == 1 { "shape" } else { "shapes" };
        let why = match &reasons[..] {
            [] => return None,
            [(_, reason)] => reason.to_string(),
            _ => reasons.iter().map(|(count, reason)| format!("{count} {reason}")).collect::<Vec<_>>().join(", "),
        };
        Some(format!("{total} {shapes} not pasted: {why}."))
    }
}

/// `clipboard` pasted on a `width` × `height` image of the project with `labels`: each copy that
/// sticks out of the image is moved just inside it, and attribute values are made to fit the label
/// as it is now. Copies bigger than the image, whose label is
/// gone, or whose label no longer allows their shape type are left out (the server would refuse
/// them).
pub fn paste(clipboard: &[Annotation], labels: &[Label], width: u32, height: u32) -> Pasted {
    let mut pasted = Pasted::default();
    for copy in clipboard {
        let Some(label) = labels.iter().find(|label| label.id == copy.label) else {
            pasted.label_deleted += 1;
            continue;
        };
        if !label.shape_types.contains(&copy.shape.shape_type()) {
            pasted.not_allowed += 1;
            continue;
        }
        match copy.shape.moved_inside(width, height).filter(|shape| shape.validate(width, height).is_ok()) {
            Some(shape) => pasted.annotations.push(Annotation { shape, attributes: label.conform_attributes(&copy.attributes), ..copy.clone() }),
            None => pasted.too_big += 1,
        }
    }
    pasted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LabelDefinition, ShapeType,
        annotation::Shape,
        attribute::Attributes,
        geometry::{Point, Rect},
    };

    /// A label allowing rectangles and polylines.
    fn label(id: i64) -> Label {
        let definition = LabelDefinition {
            name: format!("label {id}"),
            color: "#e74c3c".into(),
            hotkey: None,
            shape_types: vec![ShapeType::Rectangle, ShapeType::Polyline],
            ignore: false,
            attributes: Vec::new(),
            footprint: crate::Footprint::default(),
            show_footprint: true,
            constraints: Default::default(),
        };
        Label::new(id, definition)
    }

    fn rectangle(label: i64, x1: f64, y1: f64, x2: f64, y2: f64) -> Annotation {
        Annotation::manual(label, Shape::Rectangle(Rect::new(x1, y1, x2, y2)))
    }

    fn values(pairs: serde_json::Value) -> Attributes {
        serde_json::from_value(pairs).unwrap()
    }

    #[test]
    fn copies_are_fresh_manual_work_without_tags() {
        let predicted = Annotation {
            source: Some("yolo".into()),
            confidence: Some(0.4),
            attributes: values(serde_json::json!({ "severity": "high" })),
            hidden: true,
            locked: true,
            pending: true,
            ..Annotation::manual(3, Shape::Polyline { points: vec![Point::new(1.0, 2.0), Point::new(5.0, 6.0)] })
        };
        let tag = Annotation::manual(4, Shape::Tag);
        let copies = copied(&[predicted, tag]);
        let expected = Annotation {
            attributes: values(serde_json::json!({ "severity": "high" })),
            ..Annotation::manual(3, Shape::Polyline { points: vec![Point::new(1.0, 2.0), Point::new(5.0, 6.0)] })
        };
        assert_eq!(copies, vec![expected]);
    }

    #[test]
    fn copies_keep_their_place_or_move_just_inside_a_smaller_image() {
        let clipboard = [rectangle(1, 10.0, 10.0, 50.0, 30.0), rectangle(1, 700.0, 500.0, 900.0, 560.0)];
        let pasted = paste(&clipboard, &[label(1)], 800, 540);
        assert_eq!(pasted.annotations, vec![rectangle(1, 10.0, 10.0, 50.0, 30.0), rectangle(1, 600.0, 480.0, 800.0, 540.0)]);
    }

    #[test]
    fn copies_that_cannot_be_pasted_are_skipped_with_one_notice() {
        let ellipse = Annotation::manual(1, Shape::Ellipse(crate::ellipse::Ellipse { centre: Point::new(50.0, 50.0), rx: 10.0, ry: 5.0, angle: 0.0 }));
        let clipboard = [
            rectangle(1, 0.0, 0.0, 1000.0, 20.0),
            rectangle(2, 0.0, 0.0, 10.0, 10.0),
            ellipse,
            rectangle(1, 0.0, 0.0, 10.0, 10.0),
        ];
        let pasted = paste(&clipboard, &[label(1)], 800, 540);
        assert_eq!(pasted.annotations, vec![rectangle(1, 0.0, 0.0, 10.0, 10.0)]);
        assert_eq!(
            pasted.notice().as_deref(),
            Some("3 shapes not pasted: 1 too big for this image, 1 label deleted, 1 shape type not allowed by its label.")
        );
        let one = paste(&clipboard[..1], &[label(1)], 800, 540);
        assert_eq!(one.notice().as_deref(), Some("1 shape not pasted: too big for this image."));
        assert_eq!(paste(&clipboard[3..], &[label(1)], 800, 540).notice(), None);
    }

    #[test]
    fn attribute_values_are_made_to_fit_the_label_as_it_is_now() {
        let mut edited = label(1);
        edited.attributes = vec![crate::attribute::AttributeDefinition {
            name: "severity".into(),
            kind: crate::attribute::AttributeKind::Select { options: vec!["low".into(), "high".into()], default: "low".into() },
        }];
        let copy = Annotation { attributes: values(serde_json::json!({ "severity": "high", "removed": true })), ..rectangle(1, 0.0, 0.0, 10.0, 10.0) };
        let pasted = paste(&[copy], &[edited], 800, 540);
        assert_eq!(pasted.annotations[0].attributes, values(serde_json::json!({ "severity": "high" })));
    }
}
