//! The drawing tools: each one's press, move, release, finish and cancel, its modifiers and
//! options, and the annotations it adds.

use serde_json::json;

use super::*;
use crate::{editor::Tool, ellipse::Ellipse, keymap::Action};

fn ellipse_of(shape: Shape) -> Ellipse {
    match shape {
        Shape::Ellipse(ellipse) => ellipse,
        other => panic!("not an ellipse: {other:?}"),
    }
}

/// A session with the drawing `tool` and label 1 (which allows every shape), on no annotations.
fn drawing(tool: Tool) -> Session {
    let mut session = session(vec![]);
    session.switch_tool(tool);
    session.set_label(Some(1));
    session
}

fn with_options(mut session: Session, options: crate::editor::Options) -> Session {
    session.set_options(options);
    session
}

#[test]
fn a_dragged_rectangle_is_added_once_with_its_label() {
    let mut session = drawing(Tool::Rectangle);
    drag(&mut session, at(200.0, 200.0), at(600.0, 500.0));
    assert_eq!(session.annotations().len(), 1);
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(200.0, 200.0, 600.0, 500.0)));
    assert_eq!(session.annotations()[0].label, 1);
    assert!(selected(&session).is_empty(), "a new shape is not selected");
    assert_eq!(session.revision(), 1);
    assert!(!session.is_busy());
}

#[test]
fn two_clicks_draw_a_rectangle_and_a_click_alone_waits_for_the_second() {
    let mut session = drawing(Tool::Rectangle);
    click(&mut session, at(100.0, 100.0));
    assert!(session.is_busy(), "the first click starts the rectangle");
    assert_eq!(session.annotations().len(), 0);
    session.move_to(at(200.0, 180.0));
    assert_eq!(session.preview(), Some(crate::editor::Preview::Rectangle(Rect::new(100.0, 100.0, 200.0, 180.0))));
    click(&mut session, at(300.0, 250.0));
    assert_eq!(session.annotations().len(), 1);
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 300.0, 250.0)));
}

#[test]
fn a_click_waits_for_its_second_click_after_a_hover_and_a_foreign_release() {
    let mut session = drawing(Tool::Rectangle);
    click(&mut session, at(100.0, 100.0));
    // The pointer moves without the primary button held: that is a hover, not a drag.
    session.move_to(at(300.0, 250.0));
    // Nor does releasing another button finish the rectangle.
    session.release(at(300.0, 250.0));
    assert!(session.is_busy(), "the first click leaves the rectangle open");
    assert!(session.annotations().is_empty(), "a hover and a foreign release draw nothing");
    click(&mut session, at(300.0, 250.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 300.0, 250.0)));
}

#[test]
fn an_ellipse_click_waits_for_its_second_click_after_a_hover_and_a_foreign_release() {
    let mut session = drawing(Tool::Ellipse);
    click(&mut session, at(300.0, 300.0));
    session.move_to(at(500.0, 400.0));
    session.release(at(500.0, 400.0));
    assert!(session.is_busy(), "the first click leaves the ellipse open");
    assert!(session.annotations().is_empty());
    click(&mut session, at(500.0, 400.0));
    assert_eq!(ellipse_of(shape(&session, 0)), Ellipse { centre: Point::new(300.0, 300.0), rx: 200.0, ry: 100.0, angle: 0.0 });
}

#[test]
fn a_rectangle_too_small_on_screen_is_not_added() {
    let mut session = drawing(Tool::Rectangle);
    drag(&mut session, at(400.0, 400.0), at(410.0, 402.0));
    assert_eq!(session.annotations().len(), 0);
    assert!(!session.is_busy());
    // At 10% the same ten image pixels are one screen pixel: also too small.
    let mut session = drawing(Tool::Rectangle);
    let zoomed_out = |x, y| Pointer { scale: 0.1, ..at(x, y) };
    drag(&mut session, zoomed_out(400.0, 400.0), zoomed_out(410.0, 402.0));
    assert_eq!(session.annotations().len(), 0);
}

#[test]
fn shift_makes_a_rectangle_square_and_alt_draws_it_from_the_centre() {
    let mut session = drawing(Tool::Rectangle);
    drag(&mut session, at(100.0, 100.0), shift(at(300.0, 150.0)));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 300.0, 300.0)));
    drag(&mut session, at(500.0, 400.0), Pointer { modifiers: Modifiers { alt: true, ..Modifiers::default() }, ..at(550.0, 450.0) });
    assert_eq!(shape(&session, 1), Shape::Rectangle(Rect::new(450.0, 350.0, 550.0, 450.0)));
}

#[test]
fn an_oriented_rectangle_is_placed_with_three_points() {
    let oriented = crate::editor::Options { oriented: true, ..Default::default() };
    let mut session = with_options(drawing(Tool::Rectangle), oriented);
    click(&mut session, at(200.0, 200.0));
    click(&mut session, at(500.0, 200.0));
    click(&mut session, at(400.0, 300.0));
    assert_eq!(session.annotations().len(), 1);
    let expected = crate::oriented::OrientedRect::from_three_points(Point::new(200.0, 200.0), Point::new(500.0, 200.0), Point::new(400.0, 300.0));
    assert_eq!(shape(&session, 0), Shape::OrientedRectangle(expected));
}

#[test]
fn shift_snaps_the_second_oriented_point_to_15_degrees() {
    let oriented = crate::editor::Options { oriented: true, ..Default::default() };
    let mut session = with_options(drawing(Tool::Rectangle), oriented);
    click(&mut session, at(100.0, 100.0));
    // Ten pixels off horizontal over 300: snapped flat.
    click(&mut session, shift(at(400.0, 110.0)));
    click(&mut session, at(300.0, 300.0));
    // Flat (snapping keeps the distance), so the width axis is level with the first point.
    let Shape::OrientedRectangle(rect) = shape(&session, 0) else { panic!("not an oriented rectangle") };
    assert_eq!(rect.angle, 0.0);
    assert!((rect.centre.y - 200.0).abs() < 1e-9);
    assert!((rect.width - 300.0_f64.hypot(10.0)).abs() < 1e-9, "width {}", rect.width);
    assert_eq!(rect.height, 200.0);
}

#[test]
fn a_polygon_is_finished_with_enter_and_closes_on_its_first_vertex() {
    let mut session = drawing(Tool::Polygon);
    for (x, y) in [(100.0, 100.0), (400.0, 120.0), (300.0, 400.0)] {
        click(&mut session, at(x, y));
    }
    assert!(session.key(Action::Finish));
    assert_eq!(
        shape(&session, 0),
        Shape::Polygon { points: vec![Point::new(100.0, 100.0), Point::new(400.0, 120.0), Point::new(300.0, 400.0)] }
    );

    // A click on the first vertex closes it without adding a fourth.
    let mut session = drawing(Tool::Polygon);
    for (x, y) in [(100.0, 100.0), (400.0, 120.0), (300.0, 400.0), (101.0, 101.0)] {
        click(&mut session, at(x, y));
    }
    assert_eq!(session.annotations().len(), 1);
    assert_eq!(shape(&session, 0).vertices().len(), 3);
}

#[test]
fn a_shape_with_too_few_vertices_is_not_added() {
    let mut session = drawing(Tool::Polygon);
    click(&mut session, at(100.0, 100.0));
    click(&mut session, at(200.0, 100.0));
    assert!(session.key(Action::Finish));
    assert_eq!(session.annotations().len(), 0);
    assert!(session.take_notice().is_some_and(|notice| notice.starts_with("Not added:")));
    assert!(!session.is_busy());
}

#[test]
fn backspace_drops_the_last_vertex_and_enter_finishes_the_points() {
    let mut session = drawing(Tool::Points);
    for (x, y) in [(100.0, 100.0), (200.0, 150.0), (300.0, 200.0)] {
        click(&mut session, at(x, y));
    }
    assert_eq!(session.preview(), Some(crate::editor::Preview::Path {
        points: vec![Point::new(100.0, 100.0), Point::new(200.0, 150.0), Point::new(300.0, 200.0)],
        current: Point::new(300.0, 200.0),
        shape: crate::ShapeType::Points,
    }));
    assert!(session.key(Action::RemoveLastPoint));
    session.move_to(at(400.0, 300.0));
    assert!(session.key(Action::Finish));
    assert_eq!(shape(&session, 0), Shape::Points { points: vec![Point::new(100.0, 100.0), Point::new(200.0, 150.0)] });
}

#[test]
fn a_polyline_snaps_to_15_degrees_with_shift_or_the_snap_option() {
    let mut session = drawing(Tool::Polyline);
    click(&mut session, at(100.0, 300.0));
    click(&mut session, shift(at(400.0, 310.0)));
    assert!(session.key(Action::Finish));
    let snapped = shape(&session, 0).vertices()[1];
    assert_eq!(snapped.y, 300.0, "level with the first point");
    assert!((snapped.x - 300.0_f64.hypot(10.0) - 100.0).abs() < 1e-9, "x {}", snapped.x);

    let snap = crate::editor::Options { snap: true, ..Default::default() };
    let mut session = with_options(drawing(Tool::Polyline), snap);
    click(&mut session, at(100.0, 300.0));
    click(&mut session, at(400.0, 310.0));
    assert!(session.key(Action::Finish));
    assert_eq!(shape(&session, 0).vertices()[1].y, 300.0);
}

#[test]
fn single_point_mode_makes_each_click_its_own_annotation() {
    let single = crate::editor::Options { single_point: true, ..Default::default() };
    let mut session = with_options(drawing(Tool::Points), single);
    click(&mut session, at(100.0, 100.0));
    click(&mut session, at(300.0, 200.0));
    assert_eq!(session.annotations().len(), 2);
    assert_eq!(shape(&session, 0), Shape::Points { points: vec![Point::new(100.0, 100.0)] });
    assert_eq!(shape(&session, 1), Shape::Points { points: vec![Point::new(300.0, 200.0)] });
}

#[test]
fn a_freehand_polyline_places_vertices_every_spacing() {
    let freehand = crate::editor::Options { freehand: true, spacing: 50.0, ..Default::default() };
    let mut session = with_options(drawing(Tool::Polyline), freehand);
    session.press(at(100.0, 300.0));
    session.move_to(at(400.0, 300.0));
    session.release(at(400.0, 300.0));
    assert_eq!(session.annotations().len(), 1);
    let Shape::Polyline { points } = shape(&session, 0) else { panic!("not a polyline") };
    let xs: Vec<f64> = points.iter().map(|point| point.x).collect();
    assert_eq!(xs, [100.0, 150.0, 200.0, 250.0, 300.0, 350.0, 400.0]);
    assert!(points.iter().all(|point| point.y == 300.0));
}

#[test]
fn a_freehand_polyline_follows_edges_unless_shift_is_held() {
    let freehand = crate::editor::Options { freehand: true, spacing: 50.0, edge_snap: true, ..Default::default() };
    // A detour down and back: the session only supplies the last vertex and the raw target.
    let detour = |from: Option<Point>, _to: Point| {
        Some(match from {
            None => vec![Point::new(100.0, 300.0)],
            Some(from) => vec![from, Point::new(from.x, 500.0), Point::new(400.0, 500.0)],
        })
    };
    let mut session = with_options(drawing(Tool::Polyline), freehand);
    let layer = crate::editor::Layer { edges: Some(&detour), ..crate::editor::Layer::default() };
    session.press_in(at(100.0, 300.0), layer);
    session.move_to_in(at(400.0, 300.0), layer);
    session.release(at(400.0, 300.0));
    let Shape::Polyline { points } = shape(&session, 0) else { panic!("not a polyline") };
    assert!(points.contains(&Point::new(100.0, 500.0)), "followed the detour: {points:?}");
    assert_eq!(*points.last().unwrap(), Point::new(400.0, 500.0));

    // Shift draws freely, past the edge path.
    let mut session = with_options(drawing(Tool::Polyline), freehand);
    let layer = crate::editor::Layer { edges: Some(&detour), ..crate::editor::Layer::default() };
    session.press_in(shift(at(100.0, 300.0)), layer);
    session.move_to_in(shift(at(400.0, 300.0)), layer);
    session.release(shift(at(400.0, 300.0)));
    let Shape::Polyline { points } = shape(&session, 0) else { panic!("not a polyline") };
    assert!(points.iter().all(|point| point.y == 300.0), "a straight line: {points:?}");
}

#[test]
fn an_ellipse_is_drawn_from_its_centre_or_across_its_box_and_shift_makes_a_circle() {
    let mut session = drawing(Tool::Ellipse);
    drag(&mut session, at(500.0, 500.0), at(600.0, 550.0));
    assert_eq!(ellipse_of(shape(&session, 0)), Ellipse { centre: Point::new(500.0, 500.0), rx: 100.0, ry: 50.0, angle: 0.0 });

    drag(&mut session, at(300.0, 700.0), shift(at(360.0, 680.0)));
    assert_eq!(ellipse_of(shape(&session, 1)), Ellipse { centre: Point::new(300.0, 700.0), rx: 60.0, ry: 60.0, angle: 0.0 });

    // Near the left edge the circle shrinks around its centre and stays a circle.
    drag(&mut session, at(30.0, 500.0), shift(at(90.0, 520.0)));
    assert_eq!(ellipse_of(shape(&session, 2)), Ellipse { centre: Point::new(30.0, 500.0), rx: 30.0, ry: 30.0, angle: 0.0 });

    let boxed = crate::editor::Options { ellipse_from_centre: false, ..Default::default() };
    let mut session = with_options(drawing(Tool::Ellipse), boxed);
    click(&mut session, at(200.0, 200.0));
    click(&mut session, at(400.0, 300.0));
    assert_eq!(ellipse_of(shape(&session, 0)), Ellipse { centre: Point::new(300.0, 250.0), rx: 100.0, ry: 50.0, angle: 0.0 });
}

#[test]
fn an_ellipse_too_small_on_screen_is_not_added() {
    let mut session = drawing(Tool::Ellipse);
    // Long enough to be a drag, but its smaller radius is one screen pixel.
    drag(&mut session, at(400.0, 400.0), at(500.0, 401.0));
    assert_eq!(session.annotations().len(), 0);
    assert!(!session.is_busy());
}

#[test]
fn a_shape_with_a_constraint_is_placed_by_one_click_and_oriented_by_a_second() {
    let mut rectangle = session(vec![]);
    rectangle.switch_tool(Tool::Rectangle);
    rectangle.set_label(Some(2));
    click(&mut rectangle, at(500.0, 300.0));
    assert_eq!(shape(&rectangle, 0), Shape::Rectangle(Rect::new(450.0, 275.0, 550.0, 325.0)));
    assert!(!rectangle.is_busy(), "a rectangle needs no angle");

    let oriented = crate::editor::Options { oriented: true, ..Default::default() };
    let mut turning = with_options(session(vec![]), oriented);
    turning.switch_tool(Tool::Rectangle);
    turning.set_label(Some(2));
    click(&mut turning, at(500.0, 300.0));
    assert!(turning.is_busy(), "the centre is fixed, the angle follows");
    turning.move_to(at(600.0, 300.0));
    click(&mut turning, at(600.0, 300.0));
    assert_eq!(
        shape(&turning, 0),
        Shape::OrientedRectangle(crate::oriented::OrientedRect { centre: Point::new(500.0, 300.0), width: 100.0, height: 50.0, angle: 0.0 })
    );

    let mut round = session(vec![]);
    round.switch_tool(Tool::Ellipse);
    round.set_label(Some(2));
    click(&mut round, at(500.0, 300.0));
    click(&mut round, at(600.0, 300.0));
    assert_eq!(ellipse_of(shape(&round, 0)), Ellipse { centre: Point::new(500.0, 300.0), rx: 50.0, ry: 25.0, angle: 0.0 });
}

#[test]
fn a_shape_constraint_that_cannot_fit_is_reported_and_nothing_is_drawn() {
    let huge = label_with(4, json!({ "rectangle": { "width": 2000.0, "height": 50.0 } }));
    let mut session = Session::new(vec![], vec![huge], IMAGE);
    session.switch_tool(Tool::Rectangle);
    session.set_label(Some(4));
    click(&mut session, at(500.0, 300.0));
    assert_eq!(session.annotations().len(), 0);
    assert!(session.take_notice().is_some_and(|notice| notice.ends_with("does not fit this image.")));
}

#[test]
fn new_annotations_get_their_labels_default_attribute_values() {
    let definitions = json!([
        { "name": "severity", "type": "select", "options": ["low", "high"], "default": "high" },
        { "name": "occluded", "type": "checkbox", "default": false },
    ]);
    let label = label_with_attributes(5, &["rectangle"], definitions);
    let mut session = Session::new(vec![], vec![label], IMAGE);
    session.switch_tool(Tool::Rectangle);
    session.set_label(Some(5));
    drag(&mut session, at(100.0, 100.0), at(300.0, 200.0));
    let attributes = &session.annotations()[0].attributes;
    assert_eq!(attributes.get("severity"), Some(&crate::attribute::AttributeValue::Text("high".into())));
    assert_eq!(attributes.get("occluded"), Some(&crate::attribute::AttributeValue::Bool(false)));
}

#[test]
fn a_label_that_does_not_allow_the_shape_adds_nothing() {
    let polygons = label_with_attributes(6, &["polygon"], json!([]));
    let rectangles = label_with_attributes(7, &["rectangle"], json!([]));
    let mut session = Session::new(vec![], vec![rectangles, polygons], IMAGE);
    session.switch_tool(Tool::Rectangle);
    // The label was changed to one that no longer allows the tool's shape.
    session.set_label(Some(6));
    drag(&mut session, at(100.0, 100.0), at(300.0, 200.0));
    assert_eq!(session.annotations().len(), 0);
    assert_eq!(session.take_notice().as_deref(), Some("“label 6” cannot be used for this shape."));
}

#[test]
fn escape_cancels_the_shape_then_the_selection_then_returns_to_select() {
    let mut session = session(vec![boxed(100.0, 100.0, 200.0, 200.0)]);
    session.switch_tool(Tool::Polygon);
    session.set_label(Some(1));
    click(&mut session, at(100.0, 100.0));
    click(&mut session, at(300.0, 100.0));
    click(&mut session, at(100.0, 300.0));
    session.set_selection([0].into());
    assert!(session.key(Action::Cancel), "first the shape being drawn");
    assert!(!session.is_busy());
    assert_eq!(session.annotations().len(), 1);
    assert_eq!(selected(&session), [0], "the selection stays for the next Esc");
    assert!(session.key(Action::Cancel), "then the selection");
    assert!(selected(&session).is_empty());
    assert!(session.key(Action::Cancel), "then back to Select");
    assert_eq!(session.tool(), Tool::Select);
    assert!(!session.key(Action::Cancel), "nothing is left to cancel in Select");
}

#[test]
fn escape_drops_every_kind_of_drawing_gesture() {
    let oriented = crate::editor::Options { oriented: true, ..Default::default() };
    let freehand = crate::editor::Options { freehand: true, ..Default::default() };
    let cases = [
        (Tool::Rectangle, crate::editor::Options::default()),
        (Tool::Rectangle, oriented),
        (Tool::Polygon, crate::editor::Options::default()),
        (Tool::Polyline, crate::editor::Options::default()),
        (Tool::Polyline, freehand),
        (Tool::Points, crate::editor::Options::default()),
        (Tool::Ellipse, crate::editor::Options::default()),
    ];
    for (tool, options) in cases {
        let mut session = with_options(drawing(tool), options);
        session.press(at(100.0, 100.0));
        session.move_to(at(300.0, 250.0));
        assert!(session.is_busy(), "{tool:?} started");
        assert!(session.key(Action::Cancel));
        assert!(!session.is_busy(), "{tool:?} cancelled");
        session.release(at(300.0, 250.0));
        assert!(session.annotations().is_empty(), "{tool:?} added nothing");
    }

    // A constrained shape waiting for its angle is cancelled the same way.
    let mut session = with_options(session(vec![]), oriented);
    session.switch_tool(Tool::Rectangle);
    session.set_label(Some(2));
    click(&mut session, at(500.0, 300.0));
    assert!(session.is_busy());
    assert!(session.key(Action::Cancel));
    assert!(!session.is_busy());
    assert!(session.annotations().is_empty());
}

#[test]
fn changing_tools_drops_the_shape_being_drawn() {
    let mut session = drawing(Tool::Rectangle);
    session.press(at(100.0, 100.0));
    session.switch_tool(Tool::Polygon);
    assert!(!session.is_busy());
    session.release(at(300.0, 200.0));
    assert_eq!(session.annotations().len(), 0);
}

#[test]
fn the_rectangle_options_square_or_centre_and_shift_alt_invert_them() {
    let square = crate::editor::Options { square: true, ..Default::default() };
    let mut session = with_options(drawing(Tool::Rectangle), square);
    drag(&mut session, at(100.0, 100.0), at(300.0, 150.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 300.0, 300.0)));
    // Shift inverts the option: a free rectangle.
    drag(&mut session, at(400.0, 400.0), shift(at(600.0, 450.0)));
    assert_eq!(shape(&session, 1), Shape::Rectangle(Rect::new(400.0, 400.0, 600.0, 450.0)));

    let centred = crate::editor::Options { from_centre: true, ..Default::default() };
    let mut session = with_options(drawing(Tool::Rectangle), centred);
    drag(&mut session, at(500.0, 400.0), at(550.0, 450.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(450.0, 350.0, 550.0, 450.0)));
    drag(&mut session, at(500.0, 400.0), Pointer { modifiers: Modifiers { alt: true, ..Modifiers::default() }, ..at(550.0, 450.0) });
    assert_eq!(shape(&session, 1), Shape::Rectangle(Rect::new(500.0, 400.0, 550.0, 450.0)));
}