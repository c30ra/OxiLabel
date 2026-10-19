//! Oriented (rotated) rectangles, drawn with three points: two for one side, a third for the
//! height. Stored as centre, side lengths and angle so they stay exact rectangles under editing.

use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Rect, bounds_of};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OrientedRect {
    pub centre: Point,
    /// Length of the side along `angle`.
    pub width: f64,
    pub height: f64,
    /// Direction of the width side in radians, measured like `atan2(dy, dx)` in image coordinates
    /// (y grows downwards, so positive angles turn clockwise on screen).
    pub angle: f64,
}

impl OrientedRect {
    /// The rectangle with one side from `a` to `b` and the opposite side through `c`.
    pub fn from_three_points(a: Point, b: Point, c: Point) -> Self {
        let angle = (b.y - a.y).atan2(b.x - a.x);
        let (_, across) = axes(angle);
        // Signed distance of `c` from the line through a and b, along the side's normal.
        let height = (c.x - a.x) * across.x + (c.y - a.y) * across.y;
        let width = a.distance(b);
        let centre = Point::new(
            (a.x + b.x) / 2.0 + across.x * height / 2.0,
            (a.y + b.y) / 2.0 + across.y * height / 2.0,
        );
        OrientedRect { centre, width, height: height.abs(), angle }
    }

    /// Corners in drawing order: start of the width side, its end, then the far side back.
    pub fn corners(&self) -> [Point; 4] {
        let (along, across) = axes(self.angle);
        let (u, v) = ((along.x * self.width / 2.0, along.y * self.width / 2.0), (across.x * self.height / 2.0, across.y * self.height / 2.0));
        let c = self.centre;
        [
            Point::new(c.x - u.0 - v.0, c.y - u.1 - v.1),
            Point::new(c.x + u.0 - v.0, c.y + u.1 - v.1),
            Point::new(c.x + u.0 + v.0, c.y + u.1 + v.1),
            Point::new(c.x - u.0 + v.0, c.y - u.1 + v.1),
        ]
    }

    pub fn bounds(&self) -> Rect {
        bounds_of(&self.corners())
    }

    /// `point` in the rectangle's own axes, relative to its centre.
    fn local(&self, point: Point) -> (f64, f64) {
        let (along, across) = axes(self.angle);
        let (dx, dy) = (point.x - self.centre.x, point.y - self.centre.y);
        (dx * along.x + dy * along.y, dx * across.x + dy * across.y)
    }

    /// Whether `point` lies inside, allowing `margin` around the edges.
    pub fn contains(&self, point: Point, margin: f64) -> bool {
        let (x, y) = self.local(point);
        x.abs() <= self.width / 2.0 + margin && y.abs() <= self.height / 2.0 + margin
    }

    pub fn translated(&self, dx: f64, dy: f64) -> Self {
        OrientedRect { centre: self.centre.offset(dx, dy), ..*self }
    }

    /// The rectangle after dragging corner `index` (see `corners`) to `to`: the opposite corner
    /// stays put and the angle is kept.
    pub fn with_corner(&self, index: usize, to: Point) -> Self {
        let opposite = self.corners()[(index + 2) % 4];
        let (x, y) = self.local(to);
        let (ox, oy) = self.local(opposite);
        OrientedRect { centre: opposite.midpoint(to), width: (x - ox).abs(), height: (y - oy).abs(), angle: self.angle }
    }

    /// The same rectangle turned to `angle` around its centre.
    pub fn rotated_to(&self, angle: f64) -> Self {
        OrientedRect { angle, ..*self }
    }

    /// Where the rotation handle sits: `distance` beyond the middle of the first width side.
    pub fn rotation_handle(&self, distance: f64) -> Point {
        let (_, across) = axes(self.angle);
        let reach = self.height / 2.0 + distance;
        Point::new(self.centre.x - across.x * reach, self.centre.y - across.y * reach)
    }

    /// The angle that puts the rotation handle under `pointer`.
    pub fn angle_towards(&self, pointer: Point) -> f64 {
        // The handle points against the `across` axis, a quarter turn before `angle`.
        (pointer.y - self.centre.y).atan2(pointer.x - self.centre.x) + std::f64::consts::FRAC_PI_2
    }
}

/// Unit vectors along the width side and across it (a quarter turn clockwise on screen).
pub(crate) fn axes(angle: f64) -> (Point, Point) {
    let (sin, cos) = angle.sin_cos();
    (Point::new(cos, sin), Point::new(-sin, cos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn three_points_give_the_side_and_the_height() {
        let rect = OrientedRect::from_three_points(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(3.0, 4.0));
        assert!(close(rect.centre, Point::new(5.0, 2.0)));
        assert_eq!((rect.width, rect.height, rect.angle), (10.0, 4.0, 0.0));
        let corners = rect.corners();
        assert!(close(corners[0], Point::new(0.0, 0.0)) && close(corners[2], Point::new(10.0, 4.0)));
        // The third point on the other side of the first edge works the same way.
        let above = OrientedRect::from_three_points(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(3.0, -4.0));
        assert!(close(above.centre, Point::new(5.0, -2.0)));
        assert_eq!(above.height, 4.0);
    }

    #[test]
    fn a_rotated_rectangle_has_rotated_corners() {
        // First side from (0,0) to (0,10): pointing down the image, angle 90°.
        let rect = OrientedRect::from_three_points(Point::new(0.0, 0.0), Point::new(0.0, 10.0), Point::new(-4.0, 5.0));
        assert!((rect.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let corners = rect.corners();
        for expected in [Point::new(0.0, 0.0), Point::new(0.0, 10.0), Point::new(-4.0, 10.0), Point::new(-4.0, 0.0)] {
            assert!(corners.iter().any(|corner| close(*corner, expected)), "{expected:?} in {corners:?}");
        }
        let bounds = rect.bounds();
        assert!(close(bounds.min, Point::new(-4.0, 0.0)) && close(bounds.max, Point::new(0.0, 10.0)), "{bounds:?}");
    }

    #[test]
    fn containment_follows_the_rotation() {
        let rect = OrientedRect { centre: Point::new(0.0, 0.0), width: 20.0, height: 2.0, angle: std::f64::consts::FRAC_PI_4 };
        assert!(rect.contains(Point::new(5.0, 5.0), 0.0));
        assert!(!rect.contains(Point::new(5.0, -5.0), 0.0));
        assert!(rect.contains(Point::new(5.5, 4.5), 0.5));
    }

    #[test]
    fn dragging_a_corner_keeps_the_opposite_one_and_the_angle() {
        let rect = OrientedRect::from_three_points(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(0.0, 4.0));
        let resized = rect.with_corner(2, Point::new(14.0, 6.0));
        assert!(close(resized.corners()[0], Point::new(0.0, 0.0)));
        assert!(close(resized.corners()[2], Point::new(14.0, 6.0)));
        assert_eq!(resized.angle, rect.angle);
    }

    #[test]
    fn the_rotation_handle_turns_the_rectangle_towards_the_pointer() {
        let rect = OrientedRect { centre: Point::new(0.0, 0.0), width: 10.0, height: 4.0, angle: 0.0 };
        // Unrotated, the handle is straight above the centre.
        assert!(close(rect.rotation_handle(6.0), Point::new(0.0, -8.0)));
        assert!(rect.angle_towards(Point::new(0.0, -8.0)).abs() < 1e-12);
        // Pointer to the right: a quarter turn clockwise.
        let turned = rect.rotated_to(rect.angle_towards(Point::new(8.0, 0.0)));
        assert!((turned.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!(close(turned.rotation_handle(6.0), Point::new(8.0, 0.0)));
    }
}
