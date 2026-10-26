//! Annotations as areas: the polygons a shape covers in image pixels, for formats that need areas
//! (YOLO segment and OBB, and detect's boxes). Shapes with corners give them exactly; brush masks,
//! and polylines and points (their export footprint, see [`Footprint`]), are traced along their
//! pixels. A traced patch has no holes (YOLO cannot express them), and pixels touching only at a
//! corner are separate patches.

use oxilabel_core::{
    Footprint,
    annotation::{Annotation, Shape},
    geometry::{Point, simplify},
    mask::Bitmap,
};

/// Corners of an ellipse's polygon.
const ELLIPSE_CORNERS: usize = 36;
/// Traced outlines lose points that lie within this many pixels of the simplified line.
const TRACE_TOLERANCE: f64 = 0.75;

/// How traced outlines are given: simplified (fewer points, for polygons) or exact (every corner,
/// for boxes that must enclose every pixel).
#[derive(Clone, Copy, PartialEq)]
pub enum Detail {
    Simplified,
    Exact,
}

/// The polygons `annotation` covers in an image of `image` size, using `footprint` for lines and
/// points; none for tags.
pub fn regions(annotation: &Annotation, footprint: &Footprint, image: (u32, u32), detail: Detail) -> Vec<Vec<Point>> {
    match &annotation.shape {
        Shape::Tag => Vec::new(),
        Shape::Polygon { points } => vec![points.clone()],
        Shape::Rectangle(_) | Shape::OrientedRectangle(_) => vec![annotation.shape.vertices()],
        Shape::Ellipse(ellipse) => vec![ellipse.outline(ELLIPSE_CORNERS)],
        Shape::Mask(mask) => traced(&mask.to_bitmap(), detail),
        Shape::Polyline { .. } | Shape::Points { .. } => {
            footprint.region(&annotation.shape, image).map(|region| traced(&region, detail)).unwrap_or_default()
        }
    }
}

/// The outline of each patch of `bitmap`, simplified unless `detail` asks for every corner (and
/// always when simplifying would leave no area, as for a single pixel).
fn traced(bitmap: &Bitmap, detail: Detail) -> Vec<Vec<Point>> {
    bitmap
        .outlines()
        .into_iter()
        .map(|outline| {
            if detail == Detail::Exact {
                return outline;
            }
            // Simplify the closed loop as a line from its first corner back to it.
            let mut closed = outline.clone();
            closed.push(outline[0]);
            let mut simple = simplify(&closed, TRACE_TOLERANCE);
            simple.pop();
            if simple.len() >= 3 { simple } else { outline }
        })
        .collect()
}
