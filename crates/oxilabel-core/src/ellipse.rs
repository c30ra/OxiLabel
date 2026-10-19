//! Ellipses: a centre, a radius along each of two axes and the angle of the first axis. A circle
//! is an ellipse with equal radii.

use serde::{Deserialize, Serialize};

use crate::{
    geometry::{Point, Rect},
    oriented::{OrientedRect, axes},
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    pub centre: Point,
    /// Radius along `angle`.
    pub rx: f64,
    /// Radius across it (a quarter turn clockwise on screen).
    pub ry: f64,
    /// Direction of the `rx` axis in radians, measured like `OrientedRect::angle`.
    pub angle: f64,
}

impl Ellipse {
    pub fn circle(centre: Point, radius: f64) -> Self {
        Ellipse { centre, rx: radius, ry: radius, angle: 0.0 }
    }

    /// The axis-aligned ellipse centred on `centre` whose bounding box has a corner at `corner`.
    pub fn around(centre: Point, corner: Point) -> Self {
        Ellipse { centre, rx: (corner.x - centre.x).abs(), ry: (corner.y - centre.y).abs(), angle: 0.0 }
    }

    /// The axis-aligned ellipse filling the box with opposite corners `a` and `b`.
    pub fn in_box(a: Point, b: Point) -> Self {
        Ellipse::around(a.midpoint(b), b)
    }

    /// This unturned ellipse shrunk around its centre until it fits a `width` × `height` image:
    /// a radius that sticks out is cut to the room left, and a circle stays a circle.
    pub fn fitted(&self, width: u32, height: u32) -> Self {
        let c = self.centre;
        let room_x = c.x.min(f64::from(width) - c.x).max(0.0);
        let room_y = c.y.min(f64::from(height) - c.y).max(0.0);
        if self.is_circle() {
            let radius = self.rx.min(room_x).min(room_y);
            Ellipse { rx: radius, ry: radius, ..*self }
        } else {
            Ellipse { rx: self.rx.min(room_x), ry: self.ry.min(room_y), ..*self }
        }
    }

    /// The box around the ellipse along its own axes.
    pub fn bounding_box(&self) -> OrientedRect {
        OrientedRect { centre: self.centre, width: 2.0 * self.rx, height: 2.0 * self.ry, angle: self.angle }
    }

    pub fn is_circle(&self) -> bool {
        self.rx == self.ry
    }

    pub fn bounds(&self) -> Rect {
        let (sin, cos) = self.angle.sin_cos();
        let half_width = (self.rx * cos).hypot(self.ry * sin);
        let half_height = (self.rx * sin).hypot(self.ry * cos);
        let c = self.centre;
        Rect::new(c.x - half_width, c.y - half_height, c.x + half_width, c.y + half_height)
    }

    /// Whether `point` lies inside, allowing about `margin` around the outline.
    pub fn contains(&self, point: Point, margin: f64) -> bool {
        let (along, across) = axes(self.angle);
        let (dx, dy) = (point.x - self.centre.x, point.y - self.centre.y);
        let (x, y) = (dx * along.x + dy * along.y, dx * across.x + dy * across.y);
        (x / (self.rx + margin)).powi(2) + (y / (self.ry + margin)).powi(2) <= 1.0
    }

    pub fn translated(&self, dx: f64, dy: f64) -> Self {
        Ellipse { centre: self.centre.offset(dx, dy), ..*self }
    }

    /// `corners` points evenly spaced (by angle) around the outline, starting at the end of the
    /// `rx` axis.
    pub fn outline(&self, corners: usize) -> Vec<Point> {
        let (along, across) = axes(self.angle);
        (0..corners)
            .map(|i| {
                let (sin, cos) = (std::f64::consts::TAU * i as f64 / corners as f64).sin_cos();
                let (x, y) = (self.rx * cos, self.ry * sin);
                Point::new(self.centre.x + along.x * x + across.x * y, self.centre.y + along.y * x + across.y * y)
            })
            .collect()
    }

    /// The handles that change the radii: the ends of the `rx` and the `ry` axis.
    pub fn radius_handles(&self) -> [Point; 2] {
        let (along, across) = axes(self.angle);
        let c = self.centre;
        [
            Point::new(c.x + along.x * self.rx, c.y + along.y * self.rx),
            Point::new(c.x + across.x * self.ry, c.y + across.y * self.ry),
        ]
    }

    /// The ellipse after dragging radius handle `axis` (0 for `rx`, 1 for `ry`) to `to`: that
    /// radius becomes the distance of `to` from the centre along its axis.
    pub fn with_radius(&self, axis: usize, to: Point) -> Self {
        let direction = axes(self.angle);
        let along = if axis == 0 { direction.0 } else { direction.1 };
        let radius = ((to.x - self.centre.x) * along.x + (to.y - self.centre.y) * along.y).abs();
        if axis == 0 { Ellipse { rx: radius, ..*self } } else { Ellipse { ry: radius, ..*self } }
    }

    /// The same ellipse turned to `angle` around its centre.
    pub fn rotated_to(&self, angle: f64) -> Self {
        Ellipse { angle, ..*self }
    }

    /// Where the rotation handle sits: `distance` beyond the outline, against the `ry` axis
    /// (above the centre when unturned), as for oriented rectangles.
    pub fn rotation_handle(&self, distance: f64) -> Point {
        let (_, across) = axes(self.angle);
        let reach = self.ry + distance;
        Point::new(self.centre.x - across.x * reach, self.centre.y - across.y * reach)
    }

    /// The angle that puts the rotation handle under `pointer`.
    pub fn angle_towards(&self, pointer: Point) -> f64 {
        (pointer.y - self.centre.y).atan2(pointer.x - self.centre.x) + std::f64::consts::FRAC_PI_2
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::*;

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn an_ellipse_comes_from_its_centre_and_a_box_corner_or_from_its_box() {
        let around = Ellipse::around(Point::new(10.0, 10.0), Point::new(4.0, 13.0));
        assert_eq!((around.rx, around.ry, around.angle), (6.0, 3.0, 0.0));
        let boxed = Ellipse::in_box(Point::new(14.0, 3.0), Point::new(4.0, 7.0));
        assert!(close(boxed.centre, Point::new(9.0, 5.0)));
        assert_eq!((boxed.rx, boxed.ry), (5.0, 2.0));
    }

    #[test]
    fn a_turned_ellipse_has_turned_bounds_handles_and_containment() {
        // rx points down the image.
        let ellipse = Ellipse { centre: Point::new(0.0, 0.0), rx: 10.0, ry: 2.0, angle: FRAC_PI_2 };
        let bounds = ellipse.bounds();
        assert!(close(bounds.min, Point::new(-2.0, -10.0)) && close(bounds.max, Point::new(2.0, 10.0)), "{bounds:?}");
        let [rx, ry] = ellipse.radius_handles();
        assert!(close(rx, Point::new(0.0, 10.0)) && close(ry, Point::new(-2.0, 0.0)), "{rx:?} {ry:?}");
        assert!(ellipse.contains(Point::new(0.0, 9.0), 0.0));
        assert!(!ellipse.contains(Point::new(9.0, 0.0), 0.0));
        assert!(ellipse.contains(Point::new(2.5, 0.0), 0.5));
    }

    #[test]
    fn the_outline_lies_on_the_ellipse() {
        let ellipse = Ellipse { centre: Point::new(5.0, 5.0), rx: 4.0, ry: 1.0, angle: 0.3 };
        let outline = ellipse.outline(16);
        assert_eq!(outline.len(), 16);
        assert!(close(outline[0], ellipse.radius_handles()[0]));
        for point in outline {
            assert!(ellipse.contains(point, 1e-9) && !ellipse.contains(point, -1e-6), "{point:?}");
        }
    }

    #[test]
    fn an_ellipse_too_big_for_the_image_shrinks_around_its_centre() {
        // 30 px of room on the left, 50 above.
        let circle = Ellipse::circle(Point::new(30.0, 50.0), 40.0).fitted(100, 100);
        assert_eq!((circle.centre, circle.rx, circle.ry), (Point::new(30.0, 50.0), 30.0, 30.0), "stays a circle");
        let ellipse = Ellipse { centre: Point::new(30.0, 50.0), rx: 40.0, ry: 40.0, angle: 0.0 };
        let wide = Ellipse { rx: 40.0, ry: 20.0, ..ellipse }.fitted(100, 100);
        assert_eq!((wide.rx, wide.ry), (30.0, 20.0), "only the radius that sticks out shrinks");
        let fits = Ellipse { rx: 10.0, ry: 10.0, ..ellipse };
        assert_eq!(fits.fitted(100, 100), fits);
    }

    #[test]
    fn its_bounding_box_follows_its_axes() {
        let ellipse = Ellipse { centre: Point::new(5.0, 5.0), rx: 4.0, ry: 1.0, angle: 0.3 };
        let rect = ellipse.bounding_box();
        assert_eq!((rect.centre, rect.width, rect.height, rect.angle), (ellipse.centre, 8.0, 2.0, 0.3));
    }

    #[test]
    fn dragging_a_radius_handle_changes_only_that_radius() {
        let ellipse = Ellipse { centre: Point::new(0.0, 0.0), rx: 5.0, ry: 3.0, angle: FRAC_PI_2 };
        // The rx axis points down, so only the y distance counts.
        let wider = ellipse.with_radius(0, Point::new(4.0, -8.0));
        assert_eq!((wider.rx, wider.ry), (8.0, 3.0));
        let taller = ellipse.with_radius(1, Point::new(-6.0, 1.0));
        assert!((taller.ry - 6.0).abs() < 1e-12 && taller.rx == 5.0);
    }

    #[test]
    fn the_rotation_handle_turns_the_ellipse_towards_the_pointer() {
        let ellipse = Ellipse { centre: Point::new(0.0, 0.0), rx: 5.0, ry: 3.0, angle: 0.0 };
        assert!(close(ellipse.rotation_handle(2.0), Point::new(0.0, -5.0)));
        // Pulling the handle to the right of the centre turns it a quarter clockwise.
        let turned = ellipse.rotated_to(ellipse.angle_towards(Point::new(5.0, 0.0)));
        assert!((turned.angle - FRAC_PI_2).abs() < 1e-12);
        assert!(close(turned.rotation_handle(2.0), Point::new(5.0, 0.0)));
    }
}
