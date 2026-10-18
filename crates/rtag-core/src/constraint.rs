//! Shape constraints: a label's fixed size for some of its shape types. Drawing such a shape for
//! the label places one of that size instead of dragging it out. A constraint only guides drawing;
//! annotations of another size (imported, predicted, drawn before) stay valid.

use serde::{Deserialize, Serialize};

use crate::{
    ShapeType,
    annotation::Shape,
    ellipse::Ellipse,
    geometry::{Point, Rect},
    oriented::OrientedRect,
};

/// A width and a height in original-image pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// An ellipse's two radii in original-image pixels: `rx` along its angle, `ry` across it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Radii {
    pub rx: f64,
    pub ry: f64,
}

/// The fixed sizes of a label; a shape type without one is drawn freely.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeConstraints {
    pub rectangle: Option<Size>,
    pub oriented_rectangle: Option<Size>,
    pub ellipse: Option<Radii>,
}

impl ShapeConstraints {
    /// Whether shapes of `shape_type` have a fixed size.
    pub fn fixes(&self, shape_type: ShapeType) -> bool {
        self.size(shape_type).is_some()
    }

    /// The fixed size of `shape_type` as its two numbers (width and height, or the radii), if any.
    pub fn size(&self, shape_type: ShapeType) -> Option<(f64, f64)> {
        match shape_type {
            ShapeType::Rectangle => self.rectangle.map(|s| (s.width, s.height)),
            ShapeType::OrientedRectangle => self.oriented_rectangle.map(|s| (s.width, s.height)),
            ShapeType::Ellipse => self.ellipse.map(|r| (r.rx, r.ry)),
            _ => None,
        }
    }

    /// Fixes `shape_type` to `size` (as [`size`](Self::size) gives it), or frees it with none.
    /// Types that cannot have a fixed size are left alone.
    pub fn set_size(&mut self, shape_type: ShapeType, size: Option<(f64, f64)>) {
        match shape_type {
            ShapeType::Rectangle => self.rectangle = size.map(|(width, height)| Size { width, height }),
            ShapeType::OrientedRectangle => self.oriented_rectangle = size.map(|(width, height)| Size { width, height }),
            ShapeType::Ellipse => self.ellipse = size.map(|(rx, ry)| Radii { rx, ry }),
            _ => {}
        }
    }

    /// Whether placing a `shape_type` takes a second click for its angle: turnable shapes do,
    /// except a circle, which looks the same at any angle.
    pub fn needs_angle(&self, shape_type: ShapeType) -> bool {
        match shape_type {
            ShapeType::OrientedRectangle => self.oriented_rectangle.is_some(),
            ShapeType::Ellipse => self.ellipse.is_some_and(|radii| radii.rx != radii.ry),
            _ => false,
        }
    }

    /// The fixed-size `shape_type` centred on `centre` and turned to `angle` (rectangles ignore
    /// it), moved just enough to lie inside a `width` × `height` image. None when the type is
    /// drawn freely or the shape is larger than the image.
    pub fn place(&self, shape_type: ShapeType, centre: Point, angle: f64, width: u32, height: u32) -> Option<Shape> {
        let shape = match shape_type {
            ShapeType::Rectangle => {
                let Size { width, height } = self.rectangle?;
                let (x, y) = (centre.x - width / 2.0, centre.y - height / 2.0);
                Shape::Rectangle(Rect::new(x, y, x + width, y + height))
            }
            ShapeType::OrientedRectangle => {
                let Size { width, height } = self.oriented_rectangle?;
                Shape::OrientedRectangle(OrientedRect { centre, width, height, angle })
            }
            ShapeType::Ellipse => {
                let Radii { rx, ry } = self.ellipse?;
                Shape::Ellipse(Ellipse { centre, rx, ry, angle })
            }
            _ => return None,
        };
        shape.moved_inside(width, height)
    }

    /// The constraints made sound: every size above zero.
    pub fn validated(self) -> Result<Self, String> {
        let sizes = [ShapeType::Rectangle, ShapeType::OrientedRectangle, ShapeType::Ellipse].map(|shape| self.size(shape));
        if sizes.into_iter().flatten().any(|(a, b)| !(a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0)) {
            return Err("fixed sizes must be above zero".into());
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::*;

    const WINDOW: ShapeConstraints = ShapeConstraints {
        rectangle: Some(Size { width: 120.0, height: 80.0 }),
        oriented_rectangle: Some(Size { width: 120.0, height: 80.0 }),
        ellipse: Some(Radii { rx: 60.0, ry: 40.0 }),
    };

    #[test]
    fn a_fixed_rectangle_is_centred_on_the_click() {
        let shape = WINDOW.place(ShapeType::Rectangle, Point::new(500.0, 300.0), 1.0, 1000, 800).unwrap();
        assert_eq!(shape, Shape::Rectangle(Rect::new(440.0, 260.0, 560.0, 340.0)));
    }

    #[test]
    fn a_placed_shape_near_the_edge_moves_inside_the_image() {
        let shape = WINDOW.place(ShapeType::Rectangle, Point::new(10.0, 790.0), 0.0, 1000, 800).unwrap();
        assert_eq!(shape, Shape::Rectangle(Rect::new(0.0, 720.0, 120.0, 800.0)));
        let turned = WINDOW.place(ShapeType::OrientedRectangle, Point::new(0.0, 0.0), FRAC_PI_2, 1000, 800).unwrap();
        let bounds = turned.bounds();
        assert!((bounds.min.x).abs() < 1e-9 && (bounds.min.y).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.width() - 80.0).abs() < 1e-9 && (bounds.height() - 120.0).abs() < 1e-9);
    }

    #[test]
    fn nothing_is_placed_when_the_type_is_free_or_too_big_for_the_image() {
        let free = ShapeConstraints { rectangle: None, ..WINDOW };
        assert_eq!(free.place(ShapeType::Rectangle, Point::new(50.0, 50.0), 0.0, 1000, 800), None);
        assert_eq!(WINDOW.place(ShapeType::Polygon, Point::new(50.0, 50.0), 0.0, 1000, 800), None);
        assert_eq!(WINDOW.place(ShapeType::Rectangle, Point::new(50.0, 50.0), 0.0, 100, 100), None);
    }

    #[test]
    fn turnable_shapes_ask_for_an_angle_unless_they_are_circles() {
        assert!(WINDOW.needs_angle(ShapeType::OrientedRectangle));
        assert!(WINDOW.needs_angle(ShapeType::Ellipse));
        assert!(!WINDOW.needs_angle(ShapeType::Rectangle));
        let circle = ShapeConstraints { ellipse: Some(Radii { rx: 30.0, ry: 30.0 }), ..WINDOW };
        assert!(!circle.needs_angle(ShapeType::Ellipse));
        let ellipse = WINDOW.place(ShapeType::Ellipse, Point::new(500.0, 300.0), 0.5, 1000, 800).unwrap();
        assert_eq!(ellipse, Shape::Ellipse(Ellipse { centre: Point::new(500.0, 300.0), rx: 60.0, ry: 40.0, angle: 0.5 }));
    }

    #[test]
    fn sizes_are_read_and_set_by_shape_type() {
        let mut constraints = ShapeConstraints::default();
        constraints.set_size(ShapeType::Ellipse, Some((6.0, 4.0)));
        constraints.set_size(ShapeType::Polygon, Some((1.0, 1.0)));
        assert_eq!(constraints, ShapeConstraints { ellipse: Some(Radii { rx: 6.0, ry: 4.0 }), ..Default::default() });
        assert_eq!(constraints.size(ShapeType::Ellipse), Some((6.0, 4.0)));
        assert_eq!(WINDOW.size(ShapeType::OrientedRectangle), Some((120.0, 80.0)));
        constraints.set_size(ShapeType::Ellipse, None);
        assert_eq!(constraints, ShapeConstraints::default());
    }

    #[test]
    fn sizes_must_be_above_zero() {
        assert!(WINDOW.validated().is_ok());
        assert!(ShapeConstraints { ellipse: Some(Radii { rx: 0.0, ry: 3.0 }), ..WINDOW }.validated().is_err());
        assert!(ShapeConstraints { rectangle: Some(Size { width: f64::NAN, height: 3.0 }), ..WINDOW }.validated().is_err());
    }
}
