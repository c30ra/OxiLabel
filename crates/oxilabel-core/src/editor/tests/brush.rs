//! The brush, its eraser and Ctrl inverting one for the other, the F brush-size gesture and the
//! fuzzy fill: its seed, the threshold a drag sets and its commit on release.

use std::cell::Cell;

use super::{IMAGE, at, ctrl, session};
use crate::{
    annotation::{Annotation, Shape},
    editor::{BrushMode, FillRegion, Layer, Options, Session, Tool},
    geometry::Point,
    keymap::Action,
    mask::Bitmap,
};

/// A mask annotation covering a square of `side` pixels at (`left`, `top`).
fn mask(left: u32, top: u32, side: u32) -> Annotation {
    let mut bitmap = Bitmap::default();
    bitmap.paint_square(Point::new(f64::from(left) + f64::from(side) / 2.0, f64::from(top) + f64::from(side) / 2.0), f64::from(side) / 2.0, IMAGE);
    Annotation::manual(1, Shape::Mask(bitmap.to_mask().expect("pixels")))
}

fn bitmap(annotation: &Annotation) -> Bitmap {
    match &annotation.shape {
        Shape::Mask(mask) => mask.to_bitmap(),
        _ => panic!("not a mask"),
    }
}

/// A session with the brush and label 1 (which allows every shape) on `annotations`, its brush
/// 10 pixels across.
fn brushing(annotations: Vec<Annotation>) -> Session {
    let mut session = session(annotations);
    session.switch_tool(Tool::Brush);
    session.set_label(Some(1));
    session.set_options(Options { brush_size: 10.0, ..Options::default() });
    session
}

/// A stand-in for the analysed pixels: a fill of `threshold` pixels square at the image's corner.
fn fill_of(_seed: Point, threshold: f64) -> FillRegion {
    let side = (threshold.max(1.0)) as u32;
    let mut region = Bitmap::default();
    region.paint_square(Point::new(f64::from(side) / 2.0, f64::from(side) / 2.0), f64::from(side) / 2.0, IMAGE);
    FillRegion { region, factor: 1, incomplete: false, capped: false }
}

#[test]
fn a_stroke_paints_one_mask_in_one_undo_step() {
    let mut session = brushing(Vec::new());
    session.press(at(100.0, 100.0));
    assert!(session.is_brushing());
    session.move_to(at(200.0, 100.0));
    session.release(at(200.0, 100.0));
    assert!(!session.is_brushing());
    assert_eq!(session.annotations().len(), 1);
    let painted = bitmap(&session.annotations()[0]);
    assert!(painted.get(100, 100) && painted.get(150, 100) && painted.get(200, 100), "the whole stroke");
    assert!(!painted.get(150, 150), "it is the brush, not its bounding box");
    assert_eq!(session.annotations()[0].label, 1);
    session.key(Action::Undo);
    assert!(session.annotations().is_empty(), "one stroke is one undo step");
}

#[test]
fn ctrl_inverts_the_brush_and_the_eraser() {
    // Painting with Ctrl held erases what is under the pointer.
    let mut session = brushing(vec![mask(100, 100, 20)]);
    session.press(ctrl(at(110.0, 110.0)));
    session.release(ctrl(at(110.0, 110.0)));
    assert_eq!(session.annotations().len(), 1, "no mask was added");
    assert!(!bitmap(&session.annotations()[0]).get(110, 110), "the pixels under the brush are gone");

    // The eraser with Ctrl held paints instead.
    let mut session = brushing(Vec::new());
    session.set_brush_mode(BrushMode::Erase);
    session.press(ctrl(at(300.0, 300.0)));
    session.release(ctrl(at(300.0, 300.0)));
    assert_eq!(session.annotations().len(), 1);
    assert!(bitmap(&session.annotations()[0]).get(300, 300));
}

#[test]
fn f_resizes_the_brush_and_esc_restores_its_size() {
    let mut session = brushing(Vec::new());
    assert_eq!(session.brush_size(), 10.0);
    assert!(session.begin_brush_sizing(Point::new(100.0, 100.0)));
    assert!(session.is_sizing_brush());
    session.move_to(at(150.0, 100.0));
    assert_eq!(session.brush_size(), 110.0, "10 plus twice the 50 pixels right");
    assert!(session.key(Action::Cancel));
    assert_eq!(session.brush_size(), 10.0, "Esc restores the size");
    assert!(!session.is_busy());

    // Confirming keeps the new size.
    assert!(session.begin_brush_sizing(Point::new(100.0, 100.0)));
    session.move_to(at(120.0, 100.0));
    assert!(session.key(Action::ResizeBrush));
    assert_eq!(session.brush_size(), 50.0);
    assert!(!session.is_busy());
}

#[test]
fn a_fill_grows_with_the_threshold_drag() {
    let mut session = brushing(Vec::new());
    session.set_brush_mode(BrushMode::Fill);
    session.set_options(Options { threshold: 15.0, ..Options::default() });
    let layer = Layer { fill: Some(&fill_of), ..Layer::default() };
    session.press_in(at(500.0, 500.0), layer);
    assert_eq!(session.fill_threshold(), 15.0);
    assert_eq!(session.fill_preview().expect("a preview").region.bounds().expect("pixels").width, 15);
    session.move_to_in(at(540.0, 500.0), layer);
    assert_eq!(session.fill_threshold(), 35.0, "half a level per screen pixel");
    assert_eq!(session.fill_preview().expect("a preview").region.bounds().expect("pixels").width, 35);
    assert!(session.key(Action::Cancel));
    assert!(session.annotations().is_empty(), "a cancelled fill adds nothing");
}

#[test]
fn a_fill_is_committed_when_the_button_is_released() {
    let mut session = brushing(Vec::new());
    session.set_brush_mode(BrushMode::Fill);
    session.set_options(Options { threshold: 20.0, ..Options::default() });
    let layer = Layer { fill: Some(&fill_of), ..Layer::default() };
    session.press_in(at(500.0, 500.0), layer);
    assert!(session.annotations().is_empty(), "nothing is added while the button is held");
    session.release(at(500.0, 500.0));
    assert!(!session.is_brushing());
    assert_eq!(session.annotations().len(), 1);
    let filled = bitmap(&session.annotations()[0]);
    assert!(filled.get(5, 5) && filled.get(18, 18));
    assert!(!filled.get(300, 300));
}

#[test]
fn a_fill_waits_for_the_tiles_it_reached() {
    let ready = Cell::new(false);
    let pixels = |seed: Point, threshold: f64| FillRegion { incomplete: !ready.get(), ..fill_of(seed, threshold) };
    let layer = Layer { fill: Some(&pixels), ..Layer::default() };
    let mut session = brushing(Vec::new());
    session.set_brush_mode(BrushMode::Fill);
    session.set_options(Options { threshold: 20.0, ..Options::default() });
    session.press_in(at(500.0, 500.0), layer);
    session.release(at(500.0, 500.0));
    assert!(session.annotations().is_empty(), "the tiles it still needs are not loaded");
    ready.set(true);
    session.pixels_ready(layer);
    assert_eq!(session.annotations().len(), 1, "committed once growing is complete");
}

#[test]
fn undo_during_a_stroke_drops_it() {
    let mut session = brushing(Vec::new());
    session.press(at(100.0, 100.0));
    session.release(at(100.0, 100.0));
    assert_eq!(session.annotations().len(), 1);
    session.press(at(300.0, 300.0));
    session.move_to(at(400.0, 300.0));
    assert!(session.key(Action::Undo));
    assert!(!session.is_busy(), "the list changed, so the stroke is cancelled");
    session.release(at(400.0, 300.0));
    assert!(session.annotations().is_empty(), "only the undo changed the list");
}

#[test]
fn a_brush_without_a_label_paints_nothing() {
    let mut session = session(Vec::new());
    assert!(session.switch_tool(Tool::Brush));
    // The project's labels were changed or removed since the tool was chosen.
    session.set_label(None);
    session.press(at(100.0, 100.0));
    assert!(!session.is_busy());
    assert!(session.take_notice().is_some(), "it says a label is needed");
    assert!(session.annotations().is_empty());
}

#[test]
fn shifting_inverts_stop_at_edges_mid_stroke() {
    let limit = |_seed: Point| {
        let mut bitmap = Bitmap::default();
        bitmap.paint_square(Point::new(50.0, 50.0), 50.0, IMAGE);
        bitmap
    };
    let layer = Layer { limit: Some(&limit), ..Layer::default() };
    let mut session = brushing(Vec::new());
    session.set_options(Options { stop_at_edges: true, ..Options::default() });
    session.press_in(at(50.0, 50.0), layer);
    session.move_to_in(at(200.0, 50.0), layer);
    session.release(at(200.0, 50.0));
    let painted = bitmap(&session.annotations()[0]);
    assert!(painted.bounds().expect("pixels").right() <= 101, "held inside the limit");
    assert!(!painted.get(200, 50), "the limit stopped the stroke");
}