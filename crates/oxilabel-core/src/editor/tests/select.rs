//! The Select tool: picking, box selection, moving, and editing a shape by its handles.

use std::collections::BTreeSet;

use super::*;
use crate::{
    editor::{
        Options,
        grip::{Grip, GripKind},
    },
    ellipse::Ellipse,
    keymap::Action,
    oriented::OrientedRect,
};

const NONE: [usize; 0] = [];

fn ellipse(centre: (f64, f64), rx: f64, ry: f64) -> Annotation {
    Annotation::manual(1, Shape::Ellipse(Ellipse { centre: Point::new(centre.0, centre.1), rx, ry, angle: 0.0 }))
}

fn oriented(label: i64) -> Annotation {
    Annotation::manual(label, Shape::OrientedRectangle(OrientedRect { centre: Point::new(500.0, 400.0), width: 200.0, height: 100.0, angle: 0.0 }))
}

fn kinds(grips: &[Grip]) -> Vec<GripKind> {
    grips.iter().map(|grip| grip.kind).collect()
}

fn points_of(shape: Shape) -> Vec<(f64, f64)> {
    shape.vertices().into_iter().map(|p| (p.x, p.y)).collect()
}

fn ellipse_of(shape: Shape) -> Ellipse {
    match shape {
        Shape::Ellipse(ellipse) => ellipse,
        other => panic!("not an ellipse: {other:?}"),
    }
}

fn angle_of(shape: Shape) -> f64 {
    match shape {
        Shape::OrientedRectangle(rect) => rect.angle,
        Shape::Ellipse(ellipse) => ellipse.angle,
        other => panic!("not turnable: {other:?}"),
    }
}

/// The pointer at `degrees` (0 to the right, 90 down) and `distance` from `centre`.
fn around(centre: (f64, f64), degrees: f64, distance: f64) -> Pointer {
    let (sin, cos) = degrees.to_radians().sin_cos();
    at(centre.0 + cos * distance, centre.1 + sin * distance)
}

const SQUARE: [(f64, f64); 4] = [(100.0, 100.0), (300.0, 100.0), (300.0, 300.0), (100.0, 300.0)];

#[test]
fn a_click_picks_the_topmost_shape_and_a_click_on_empty_space_clears_the_selection() {
    let mut session = session(vec![boxed(100.0, 100.0, 300.0, 300.0), boxed(200.0, 200.0, 400.0, 400.0)]);
    click(&mut session, at(250.0, 250.0));
    assert_eq!(selected(&session), [1]);
    click(&mut session, at(150.0, 150.0));
    assert_eq!(selected(&session), [0]);
    click(&mut session, at(700.0, 700.0));
    assert_eq!(selected(&session), NONE);
    assert_eq!(session.revision(), 0, "picking changes no annotation");
}

#[test]
fn shift_click_toggles_a_shape_and_ctrl_click_removes_it() {
    let mut session = session(vec![boxed(100.0, 100.0, 200.0, 200.0), boxed(300.0, 100.0, 400.0, 200.0)]);
    click(&mut session, at(150.0, 150.0));
    click(&mut session, shift(at(350.0, 150.0)));
    assert_eq!(selected(&session), [0, 1]);
    click(&mut session, shift(at(150.0, 150.0)));
    assert_eq!(selected(&session), [1]);
    click(&mut session, ctrl(at(350.0, 150.0)));
    assert_eq!(selected(&session), NONE);
}

#[test]
fn hidden_shapes_and_pre_annotations_below_the_threshold_are_not_picked() {
    let mut hidden = boxed(100.0, 100.0, 200.0, 200.0);
    hidden.hidden = true;
    let unsure = Annotation { pending: true, confidence: Some(0.3), ..boxed(300.0, 100.0, 400.0, 200.0) };
    let mut session = session(vec![hidden, unsure]);
    session.set_threshold(0.5);
    click(&mut session, at(150.0, 150.0));
    click(&mut session, shift(at(350.0, 150.0)));
    assert_eq!(selected(&session), NONE);
    session.set_threshold(0.2);
    click(&mut session, at(350.0, 150.0));
    assert_eq!(selected(&session), [1]);
    session.set_threshold(0.5);
    assert_eq!(selected(&session), NONE, "what the threshold hides cannot stay selected");
}

#[test]
fn dragging_a_selected_shape_moves_the_whole_selection_in_one_undo_step() {
    let mut session = session(vec![boxed(100.0, 100.0, 200.0, 200.0), boxed(300.0, 100.0, 400.0, 200.0), boxed(600.0, 600.0, 700.0, 700.0)]);
    click(&mut session, at(150.0, 150.0));
    click(&mut session, shift(at(350.0, 150.0)));
    session.press(at(150.0, 150.0));
    session.move_to(at(200.0, 180.0));
    // While dragging, the selection shows where it goes; nothing has changed yet.
    let first = session.annotations()[0].clone();
    assert_eq!(*session.shown_shape(0, &first), Shape::Rectangle(Rect::new(150.0, 130.0, 250.0, 230.0)));
    assert_eq!(session.revision(), 0);
    session.release(at(200.0, 180.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(150.0, 130.0, 250.0, 230.0)));
    assert_eq!(shape(&session, 1), Shape::Rectangle(Rect::new(350.0, 130.0, 450.0, 230.0)));
    assert_eq!(shape(&session, 2), Shape::Rectangle(Rect::new(600.0, 600.0, 700.0, 700.0)));
    assert_eq!(selected(&session), [0, 1], "the moved shapes stay selected");
    session.undo();
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    assert_eq!(shape(&session, 1), Shape::Rectangle(Rect::new(300.0, 100.0, 400.0, 200.0)));
}

#[test]
fn a_move_stops_at_the_image_border() {
    let mut session = session(vec![boxed(100.0, 100.0, 200.0, 200.0)]);
    drag(&mut session, at(150.0, 150.0), at(-500.0, 2000.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(0.0, 700.0, 100.0, 800.0)));
}

#[test]
fn locked_shapes_stay_put_when_the_selection_moves() {
    let mut locked = boxed(100.0, 100.0, 200.0, 200.0);
    locked.locked = true;
    let mut session = session(vec![locked, boxed(300.0, 100.0, 400.0, 200.0)]);
    session.key(Action::SelectAll);
    drag(&mut session, at(350.0, 150.0), at(400.0, 150.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    assert_eq!(shape(&session, 1), Shape::Rectangle(Rect::new(350.0, 100.0, 450.0, 200.0)));
    assert!(session.take_notice().is_some_and(|notice| notice.starts_with("Locked shapes stay")));
    // A locked shape alone has no handles to edit it.
    click(&mut session, at(700.0, 700.0));
    click(&mut session, at(150.0, 150.0));
    assert_eq!(selected(&session), [0]);
    assert_eq!(session.grips(1.0), []);
}

#[test]
fn a_selection_box_replaces_extends_or_subtracts_from_the_selection() {
    let mut session = session(vec![boxed(100.0, 100.0, 200.0, 200.0), boxed(300.0, 100.0, 400.0, 200.0), boxed(800.0, 600.0, 900.0, 700.0)]);
    session.press(at(50.0, 50.0));
    session.move_to(at(450.0, 250.0));
    assert_eq!(session.selection_box(), Some(Rect::new(50.0, 50.0, 450.0, 250.0)));
    session.release(at(450.0, 250.0));
    assert_eq!(selected(&session), [0, 1]);
    assert_eq!(session.selection_box(), None);
    drag(&mut session, shift(at(750.0, 550.0)), shift(at(950.0, 750.0)));
    assert_eq!(selected(&session), [0, 1, 2]);
    drag(&mut session, ctrl(at(50.0, 50.0)), ctrl(at(250.0, 250.0)));
    assert_eq!(selected(&session), [1, 2]);
    // A click on empty space, or a box too small to be meant, only clears the selection.
    drag(&mut session, at(500.0, 400.0), at(502.0, 402.0));
    assert_eq!(selected(&session), NONE);
}

#[test]
fn a_rectangle_is_resized_by_its_handles_but_not_below_the_smallest_size() {
    let mut session = session(vec![boxed(100.0, 100.0, 300.0, 200.0)]);
    click(&mut session, at(200.0, 150.0));
    drag(&mut session, at(300.0, 200.0), at(400.0, 250.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 400.0, 250.0)));
    // The middle of the left side moves only that side.
    drag(&mut session, at(100.0, 175.0), at(150.0, 600.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(150.0, 100.0, 400.0, 250.0)));
    // Shrunk to 1 × 1 on screen: refused.
    drag(&mut session, at(400.0, 250.0), at(151.0, 101.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(150.0, 100.0, 400.0, 250.0)));
    // At 10%, the same handle is in reach from 80 image pixels away.
    drag(&mut session, Pointer { scale: 0.1, ..at(480.0, 250.0) }, Pointer { scale: 0.1, ..at(500.0, 300.0) });
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(150.0, 100.0, 500.0, 300.0)));
    assert_eq!(session.revision(), 3);
}

#[test]
fn on_a_tiny_rectangle_every_handle_is_in_reach_and_the_nearest_one_wins() {
    use crate::geometry::{Edge, Handle};
    let mut session = session(vec![boxed(100.0, 100.0, 104.0, 104.0)]);
    click(&mut session, at(102.0, 102.0));
    let handle = |x: f64, y: f64| match session.grip_at(Point::new(x, y), 1.0).map(|grip| grip.kind) {
        Some(GripKind::Resize(handle)) => Some(handle),
        _ => None,
    };
    assert_eq!(handle(103.5, 100.5), Some(Handle { x: Edge::Max, y: Edge::Min }));
    assert_eq!(handle(104.0, 102.2), Some(Handle { x: Edge::Max, y: Edge::Neither }));
    assert_eq!(handle(120.0, 102.0), None);
}

#[test]
fn vertices_are_dragged_inserted_from_an_edge_and_removed_with_ctrl() {
    let mut session = session(vec![polygon(&SQUARE)]);
    click(&mut session, at(200.0, 200.0));
    drag(&mut session, at(300.0, 300.0), at(400.0, 350.0));
    assert_eq!(points_of(shape(&session, 0)), [(100.0, 100.0), (300.0, 100.0), (400.0, 350.0), (100.0, 300.0)]);
    // The "+" in the middle of the first edge.
    drag(&mut session, at(200.0, 100.0), at(200.0, 50.0));
    assert_eq!(points_of(shape(&session, 0)), [(100.0, 100.0), (200.0, 50.0), (300.0, 100.0), (400.0, 350.0), (100.0, 300.0)]);
    click(&mut session, ctrl(at(200.0, 50.0)));
    assert_eq!(points_of(shape(&session, 0)), [(100.0, 100.0), (300.0, 100.0), (400.0, 350.0), (100.0, 300.0)]);
    // Dragged outside the image, a vertex stops at its border.
    drag(&mut session, at(100.0, 300.0), at(-50.0, 900.0));
    assert_eq!(points_of(shape(&session, 0))[3], (0.0, 800.0));
}

#[test]
fn a_vertex_change_that_would_spoil_the_shape_is_refused() {
    let mut session = session(vec![polygon(&[(100.0, 100.0), (300.0, 100.0), (200.0, 300.0)])]);
    click(&mut session, at(200.0, 150.0));
    click(&mut session, ctrl(at(200.0, 300.0)));
    assert_eq!(session.take_notice().as_deref(), Some("That shape cannot lose another point."));
    // Onto the line through the other two: no area left.
    drag(&mut session, at(200.0, 300.0), at(200.0, 100.0));
    assert!(session.take_notice().is_some_and(|notice| notice.starts_with("Not changed:")));
    assert_eq!(points_of(shape(&session, 0)), [(100.0, 100.0), (300.0, 100.0), (200.0, 300.0)]);
    assert_eq!(session.revision(), 0);
}

#[test]
fn an_oriented_rectangle_turns_by_its_handle_in_steps_with_shift_or_the_snap_option() {
    let mut session = session(vec![oriented(1)]);
    click(&mut session, at(500.0, 400.0));
    // The handle is 24 screen pixels beyond the middle of the first width side.
    let handle = at(500.0, 400.0 - 50.0 - 24.0);
    drag(&mut session, handle, at(600.0, 400.0));
    assert!((angle_of(shape(&session, 0)) - 90f64.to_radians()).abs() < 1e-9);

    let mut session = super::session(vec![oriented(1)]);
    click(&mut session, at(500.0, 400.0));
    // The handle towards 50° from upright: Shift snaps it to 45°.
    drag(&mut session, handle, shift(around((500.0, 400.0), 50.0 - 90.0, 100.0)));
    assert!((angle_of(shape(&session, 0)) - 45f64.to_radians()).abs() < 1e-9);

    let mut session = super::session(vec![oriented(1)]);
    session.set_options(Options { snap: true });
    click(&mut session, at(500.0, 400.0));
    drag(&mut session, handle, around((500.0, 400.0), 50.0 - 90.0, 100.0));
    assert!((angle_of(shape(&session, 0)) - 45f64.to_radians()).abs() < 1e-9, "the snap option, without Shift");
}

#[test]
fn an_ellipse_radius_follows_its_handle_up_to_the_image_border() {
    let mut session = session(vec![ellipse((500.0, 400.0), 100.0, 50.0)]);
    click(&mut session, at(500.0, 400.0));
    drag(&mut session, at(600.0, 400.0), at(700.0, 420.0));
    assert_eq!(ellipse_of(shape(&session, 0)), Ellipse { centre: Point::new(500.0, 400.0), rx: 200.0, ry: 50.0, angle: 0.0 });
    // Far past the border: as wide as the image allows around the centre.
    drag(&mut session, at(700.0, 400.0), at(2000.0, 400.0));
    let wide = ellipse_of(shape(&session, 0));
    assert!(wide.rx > 499.9 && wide.rx <= 500.0, "rx {}", wide.rx);
    // Dragged onto the centre, the radius would vanish: refused.
    drag(&mut session, at(500.0, 450.0), at(500.0, 400.0));
    assert_eq!(ellipse_of(shape(&session, 0)).ry, 50.0);
}

#[test]
fn an_ellipse_turns_by_its_handle_unless_it_would_leave_the_image() {
    let mut session = session(vec![ellipse((500.0, 400.0), 100.0, 50.0)]);
    click(&mut session, at(500.0, 400.0));
    drag(&mut session, at(500.0, 400.0 - 50.0 - 24.0), at(600.0, 400.0));
    assert!((angle_of(shape(&session, 0)) - 90f64.to_radians()).abs() < 1e-9);

    // Upright, it would stick out at the top.
    let mut session = super::session(vec![ellipse((500.0, 60.0), 100.0, 50.0)]);
    click(&mut session, at(500.0, 60.0));
    drag(&mut session, at(500.0, 60.0 - 50.0 - 24.0), at(600.0, 60.0));
    assert_eq!(angle_of(shape(&session, 0)), 0.0);
    assert_eq!(session.revision(), 0);
}

#[test]
fn the_ellipse_centre_handle_moves_it() {
    let mut session = session(vec![ellipse((500.0, 400.0), 100.0, 50.0)]);
    click(&mut session, at(500.0, 400.0));
    drag(&mut session, at(500.0, 400.0), at(450.0, 300.0));
    assert_eq!(ellipse_of(shape(&session, 0)).centre, Point::new(450.0, 300.0));
}

#[test]
fn which_handles_a_shape_has() {
    let mut session = session(vec![boxed(100.0, 100.0, 300.0, 200.0), polygon(&SQUARE[..3]), ellipse((500.0, 400.0), 100.0, 50.0), oriented(1)]);
    let mut handles = |index: usize| {
        session.set_selection(BTreeSet::from([index]));
        kinds(&session.grips(1.0))
    };
    assert_eq!(handles(0).len(), 8);
    assert!(handles(0).iter().all(|kind| matches!(kind, GripKind::Resize(_))));
    assert_eq!(handles(1), [GripKind::Insert(0), GripKind::Insert(1), GripKind::Insert(2), GripKind::Vertex(0), GripKind::Vertex(1), GripKind::Vertex(2)]);
    assert!(matches!(handles(2)[..], [GripKind::Vertex(0), GripKind::Radius(0), GripKind::Radius(1), GripKind::Turn { .. }]));
    assert!(matches!(handles(3)[..], [GripKind::Vertex(0), GripKind::Vertex(1), GripKind::Vertex(2), GripKind::Vertex(3), GripKind::Turn { .. }]));
    // Only one selected shape shows its handles.
    session.set_selection(BTreeSet::from([0, 1]));
    assert_eq!(session.grips(1.0), []);
}

#[test]
fn a_shape_of_fixed_size_has_no_handles_that_resize_it() {
    let fixed = |shape| Annotation::manual(2, shape);
    let rectangle = fixed(Shape::Rectangle(Rect::new(100.0, 100.0, 200.0, 150.0)));
    let turned = oriented(2);
    let round = fixed(Shape::Ellipse(Ellipse { centre: Point::new(800.0, 100.0), rx: 50.0, ry: 25.0, angle: 0.0 }));
    let mut session = session(vec![rectangle, turned, round]);
    click(&mut session, at(800.0, 100.0));
    assert!(matches!(kinds(&session.grips(1.0))[..], [GripKind::Vertex(0), GripKind::Turn { .. }]));
    click(&mut session, at(500.0, 400.0));
    assert!(matches!(kinds(&session.grips(1.0))[..], [GripKind::Turn { .. }]));
    click(&mut session, at(150.0, 125.0));
    assert_eq!(session.grips(1.0), []);
    // Its corner is no handle: dragging there moves the rectangle.
    drag(&mut session, at(200.0, 150.0), at(250.0, 200.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(150.0, 150.0, 250.0, 200.0)));
}

#[test]
fn handles_follow_the_shape_while_it_is_edited() {
    let mut session = session(vec![polygon(&SQUARE)]);
    click(&mut session, at(200.0, 200.0));
    session.press(at(300.0, 300.0));
    session.move_to(at(400.0, 350.0));
    assert!(session.grips(1.0).contains(&Grip { kind: GripKind::Vertex(2), at: Point::new(400.0, 350.0) }));
}

#[test]
fn undo_during_a_vertex_drag_cancels_the_drag() {
    let mut session = session(vec![polygon(&SQUARE)]);
    click(&mut session, at(200.0, 200.0));
    drag(&mut session, at(300.0, 300.0), at(400.0, 350.0));
    click(&mut session, at(200.0, 200.0));
    session.press(at(100.0, 100.0));
    session.move_to(at(50.0, 50.0));
    assert!(session.key(Action::Undo));
    assert!(!session.is_busy(), "the drag is cancelled");
    session.move_to(at(60.0, 60.0));
    session.release(at(60.0, 60.0));
    assert_eq!(points_of(shape(&session, 0)), SQUARE, "only the undo changed the polygon");
    // Redo likewise.
    session.key(Action::SelectAll);
    session.press(at(100.0, 100.0));
    session.move_to(at(50.0, 50.0));
    session.key(Action::Redo);
    assert!(!session.is_busy());
    session.release(at(50.0, 50.0));
    assert_eq!(points_of(shape(&session, 0)), [(100.0, 100.0), (300.0, 100.0), (400.0, 350.0), (100.0, 300.0)]);
}

#[test]
fn a_pre_annotation_run_merged_during_a_move_cancels_the_move() {
    let run = |x: f64| Annotation { source: Some("model".into()), pending: true, confidence: Some(0.9), ..boxed(x, 500.0, x + 50.0, 550.0) };
    let mut session = session(vec![run(600.0), boxed(100.0, 100.0, 200.0, 200.0)]);
    click(&mut session, at(150.0, 150.0));
    session.press(at(150.0, 150.0));
    session.move_to(at(250.0, 150.0));
    // The run replaced its pre-annotation: the box moves to index 0.
    session.merge_pre_annotations("model", &[run(700.0)]);
    assert!(!session.is_busy(), "the move is cancelled");
    session.release(at(250.0, 150.0));
    assert_eq!(session.annotations(), [boxed(100.0, 100.0, 200.0, 200.0), run(700.0)]);
}

#[test]
fn a_run_merged_during_a_vertex_drag_cannot_change_the_wrong_annotation() {
    let run = Annotation { source: Some("model".into()), pending: true, ..boxed(600.0, 500.0, 700.0, 600.0) };
    let mut session = session(vec![run.clone(), polygon(&SQUARE), polygon(&[(500.0, 100.0), (700.0, 100.0), (700.0, 300.0)])]);
    click(&mut session, at(200.0, 200.0));
    session.press(at(300.0, 300.0));
    session.move_to(at(400.0, 350.0));
    // The run's pre-annotation is gone: the other polygon now has index 1.
    session.merge_pre_annotations("model", &[]);
    session.release(at(400.0, 350.0));
    assert_eq!(session.annotations(), [polygon(&SQUARE), polygon(&[(500.0, 100.0), (700.0, 100.0), (700.0, 300.0)])]);
}

#[test]
fn deleting_during_a_drag_cancels_it() {
    let mut session = session(vec![polygon(&SQUARE), boxed(500.0, 100.0, 600.0, 200.0)]);
    click(&mut session, at(200.0, 200.0));
    session.press(at(300.0, 300.0));
    session.move_to(at(400.0, 350.0));
    session.key(Action::Delete);
    assert!(!session.is_busy());
    session.release(at(400.0, 350.0));
    assert_eq!(session.annotations(), [boxed(500.0, 100.0, 600.0, 200.0)]);
}

#[test]
fn escape_cancels_the_gesture_then_clears_the_selection_then_is_left_to_the_tool() {
    let mut session = session(vec![boxed(100.0, 100.0, 200.0, 200.0)]);
    click(&mut session, at(150.0, 150.0));
    session.press(at(150.0, 150.0));
    session.move_to(at(250.0, 150.0));
    assert!(session.key(Action::Cancel));
    session.release(at(250.0, 150.0));
    assert_eq!(shape(&session, 0), Shape::Rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    assert_eq!(selected(&session), [0]);
    assert!(session.key(Action::Cancel));
    assert_eq!(selected(&session), NONE);
    assert!(!session.key(Action::Cancel));
}
