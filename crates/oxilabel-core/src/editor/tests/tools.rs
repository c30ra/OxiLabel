//! Choosing a tool: one session rule behind the tool keys, the tool pie, the tool rail and a tap
//! of the pie key. It cancels the gesture in progress, resets the brush to plain painting, and
//! keeps the label only while it allows the tool's shape.

use super::*;
use crate::{
    ShapeType,
    editor::{BrushMode, Tool},
};

/// A label allowing only `shapes`.
fn label_shape(id: i64, shapes: &[ShapeType]) -> Label {
    let mut label = label_with(id, serde_json::json!({}));
    label.shape_types = shapes.to_vec();
    label
}

fn ready(tool: Tool) -> Session {
    let mut session = Session::new(
        vec![boxed(100.0, 100.0, 300.0, 300.0)],
        vec![label_shape(1, &[ShapeType::Rectangle]), label_shape(2, &[ShapeType::Polyline]), label_shape(3, &ShapeType::ALL)],
        IMAGE,
    );
    session.set_tool(tool);
    session
}

#[test]
fn switching_tools_cancels_the_gesture_in_progress() {
    let mut session = session(vec![boxed(100.0, 100.0, 300.0, 300.0)]);
    click(&mut session, at(150.0, 150.0));
    session.press(at(150.0, 150.0));
    assert!(session.is_busy(), "a move is in progress");

    assert!(session.switch_tool(Tool::Brush));

    assert!(!session.is_busy(), "switching tools cancels the move");
    assert_eq!(session.tool(), Tool::Brush);
}

#[test]
fn switching_keeps_a_label_that_allows_the_shape_and_takes_one_that_does() {
    let mut session = ready(Tool::Select);
    session.set_label(Some(1));

    assert!(session.switch_tool(Tool::Rectangle));
    assert_eq!(session.label(), Some(1), "label 1 allows rectangles");

    assert!(session.switch_tool(Tool::Polyline));
    assert_eq!(session.label(), Some(2), "label 1 does not allow polylines: label 2 does");

    assert!(session.switch_tool(Tool::Brush));
    assert_eq!(session.label(), Some(3), "only label 3 allows masks");
}

#[test]
fn a_tool_no_label_allows_is_refused_with_a_notice() {
    let mut session = Session::new(Vec::new(), vec![label_shape(1, &[ShapeType::Rectangle])], IMAGE);
    session.set_label(Some(1));

    assert!(!session.switch_tool(Tool::Brush));

    assert_eq!(session.tool(), Tool::Select, "the tool stays");
    assert_eq!(session.label(), Some(1), "the label stays");
    assert_eq!(
        session.take_notice().as_deref(),
        Some("No label allows brush shapes: create or edit labels on the Labels tab.")
    );
}

// The four entry points all call `switch_tool`; each test starts from the tool and brush mode the
// entry point leaves behind.

#[test]
fn the_brush_key_starts_plain_painting() {
    let mut session = ready(Tool::Select);
    session.set_brush_mode(BrushMode::Erase);

    assert!(session.switch_tool(Tool::Brush));

    assert_eq!((session.tool(), session.brush_mode(), session.label()), (Tool::Brush, BrushMode::Paint, Some(3)));
}

#[test]
fn the_tool_pie_starts_plain_painting() {
    let mut session = ready(Tool::Rectangle);
    session.set_brush_mode(BrushMode::Fill);

    assert!(session.switch_tool(Tool::Brush));

    assert_eq!((session.tool(), session.brush_mode(), session.label()), (Tool::Brush, BrushMode::Paint, Some(3)));
}

#[test]
fn the_tool_rail_starts_plain_painting() {
    let mut session = ready(Tool::Ellipse);
    session.set_brush_mode(BrushMode::FillErase);

    assert!(session.switch_tool(Tool::Brush));

    assert_eq!((session.tool(), session.brush_mode(), session.label()), (Tool::Brush, BrushMode::Paint, Some(3)));
}

#[test]
fn a_tap_of_the_pie_key_starts_plain_painting() {
    let mut session = ready(Tool::Select);
    assert!(session.switch_tool(Tool::Brush));
    session.set_brush_mode(BrushMode::Erase);
    assert!(session.switch_tool(Tool::Select));

    // The tap repeats the kind of tool: a brush starts plain painting again.
    assert!(session.switch_tool(Tool::Brush));

    assert_eq!((session.tool(), session.brush_mode(), session.label()), (Tool::Brush, BrushMode::Paint, Some(3)));
}

#[test]
fn erasing_keeps_fuzzy_fill_and_the_options_keep_the_other_switch() {
    let mut session = ready(Tool::Brush);
    session.set_brush_mode(BrushMode::Fill);

    session.set_brush_eraser(true);
    assert_eq!(session.brush_mode(), BrushMode::FillErase, "the eraser key keeps fuzzy fill");

    session.set_brush_fill(false);
    assert_eq!(session.brush_mode(), BrushMode::Erase, "turning fill off keeps the eraser");

    session.set_brush_fill(true);
    assert_eq!(session.brush_mode(), BrushMode::FillErase);
}

#[test]
fn choosing_another_tool_keeps_the_brush_mode() {
    let mut session = ready(Tool::Brush);
    session.set_brush_mode(BrushMode::Erase);

    assert!(session.switch_tool(Tool::Rectangle));

    assert_eq!(session.tool(), Tool::Rectangle);
    assert_eq!(session.brush_mode(), BrushMode::Erase, "only choosing the brush resets it");
}