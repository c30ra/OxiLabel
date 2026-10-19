//! Annotations as areas: the polygons a shape covers in image pixels, for formats that need areas
//! (YOLO segment and OBB, and detect's boxes). Shapes with corners give them exactly; brush masks,
//! and polylines and points drawn with their footprint (line width, point radius and shape), are
//! traced along their pixels. A square point covers the pixels whose centres lie in the square.
//!
//! Lines and points are drawn as the author's `cvat_to_masks.py` draws them with OpenCV:
//! coordinates rounded to whole pixels and taken as pixel centres, the line width (at least 1)
//! and point radius cut to whole pixels, a radius of 0 still drawing the one pixel. OpenCV's thick
//! lines come out wider than their thickness t: 2·⌈t/2⌉ + 1 pixels across when level or upright,
//! drawn here as a round-ended stroke of half-width ⌈t/2⌉ + ½ (½ for a 1-pixel line). At other
//! angles OpenCV's width wanders by about a pixel either way, which this does not follow. Its filled
//! circles of radius r are the pixels whose centres lie within r. A traced patch
//! has no holes (YOLO cannot express them), and pixels touching only at a corner are separate
//! patches.

use oxilabel_core::{
    Footprint, PointShape,
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
    // OpenCV's coordinates: whole pixels, at the pixel's centre.
    let pixel = |p: &Point| Point::new(p.x.round() + 0.5, p.y.round() + 0.5);
    match &annotation.shape {
        Shape::Tag => Vec::new(),
        Shape::Polygon { points } => vec![points.clone()],
        Shape::Rectangle(_) | Shape::OrientedRectangle(_) => vec![annotation.shape.vertices()],
        Shape::Ellipse(ellipse) => vec![ellipse.outline(ELLIPSE_CORNERS)],
        Shape::Mask(mask) => traced(&mask.to_bitmap(), detail),
        Shape::Polyline { points } => {
            let radius = footprint.line_reach();
            let mut bitmap = Bitmap::default();
            // A one-point line is a dot.
            let ends: Vec<Point> = points.iter().map(pixel).collect();
            match &ends[..] {
                [dot] => {
                    bitmap.paint(*dot, *dot, radius, image);
                }
                _ => ends.windows(2).for_each(|pair| {
                    bitmap.paint(pair[0], pair[1], radius, image);
                }),
            }
            traced(&bitmap, detail)
        }
        Shape::Points { points } => {
            let radius = footprint.point_reach();
            let mut bitmap = Bitmap::default();
            for point in points.iter().map(pixel) {
                match footprint.point_shape {
                    PointShape::Disc => bitmap.paint(point, point, radius, image),
                    PointShape::Square => bitmap.paint_square(point, radius, image),
                };
            }
            traced(&bitmap, detail)
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
