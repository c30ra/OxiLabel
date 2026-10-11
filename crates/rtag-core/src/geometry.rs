//! Points and rectangles. Image coordinates are in original-image pixels, with the origin at the
//! top-left corner of the top-left pixel.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    pub fn distance(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    pub fn offset(self, dx: f64, dy: f64) -> Point {
        Point::new(self.x + dx, self.y + dy)
    }

    pub fn midpoint(self, other: Point) -> Point {
        Point::new((self.x + other.x) / 2.0, (self.y + other.y) / 2.0)
    }

    /// The same point kept inside a `width` × `height` image.
    pub fn clamped(self, width: f64, height: f64) -> Point {
        Point::new(self.x.clamp(0.0, width), self.y.clamp(0.0, height))
    }
}

/// `to`, moved so the direction from `from` is a multiple of `step_degrees` (Shift while drawing
/// lines). The distance along that direction is kept.
pub fn snap_angle(from: Point, to: Point, step_degrees: f64) -> Point {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = dx.hypot(dy);
    if length == 0.0 {
        return to;
    }
    let step = step_degrees.to_radians();
    let angle = (dy.atan2(dx) / step).round() * step;
    Point::new(from.x + length * angle.cos(), from.y + length * angle.sin())
}

/// Shortest distance from `point` to the segment from `a` to `b`.
pub fn distance_to_segment(point: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_squared = dx * dx + dy * dy;
    if length_squared == 0.0 {
        return point.distance(a);
    }
    let t = (((point.x - a.x) * dx + (point.y - a.y) * dy) / length_squared).clamp(0.0, 1.0);
    point.distance(Point::new(a.x + t * dx, a.y + t * dy))
}

/// Whether `point` is inside the polygon with these vertices (even-odd rule).
pub fn inside_polygon(point: Point, vertices: &[Point]) -> bool {
    let mut inside = false;
    let mut previous = match vertices.last() {
        Some(last) => *last,
        None => return false,
    };
    for &current in vertices {
        if (current.y > point.y) != (previous.y > point.y)
            && point.x < (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y) + current.x
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

/// Area enclosed by the polygon (shoelace formula), whatever the vertex order.
pub fn polygon_area(vertices: &[Point]) -> f64 {
    let n = vertices.len();
    (0..n).map(|i| {
        let (a, b) = (vertices[i], vertices[(i + 1) % n]);
        a.x * b.y - b.x * a.y
    })
    .sum::<f64>()
    .abs()
        / 2.0
}

/// The smallest axis-aligned rectangle around `points`.
pub fn bounds_of(points: &[Point]) -> Rect {
    let mut bounds = Rect::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in points {
        bounds.min.x = bounds.min.x.min(point.x);
        bounds.min.y = bounds.min.y.min(point.y);
        bounds.max.x = bounds.max.x.max(point.x);
        bounds.max.y = bounds.max.y.max(point.y);
    }
    bounds
}

/// An axis-aligned rectangle from `min` (inclusive) to `max` (exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub const fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Rect { min: Point::new(min_x, min_y), max: Point::new(max_x, max_y) }
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
}

/// Which edge of a rectangle a handle moves along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Min,
    Max,
    /// The handle sits in the middle of this axis and leaves it alone.
    Neither,
}

/// One of the eight resize handles: corners move two edges, side midpoints one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle {
    pub x: Edge,
    pub y: Edge,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle { x: Edge::Min, y: Edge::Min },
        Handle { x: Edge::Neither, y: Edge::Min },
        Handle { x: Edge::Max, y: Edge::Min },
        Handle { x: Edge::Max, y: Edge::Neither },
        Handle { x: Edge::Max, y: Edge::Max },
        Handle { x: Edge::Neither, y: Edge::Max },
        Handle { x: Edge::Min, y: Edge::Max },
        Handle { x: Edge::Min, y: Edge::Neither },
    ];

    /// CSS cursor showing the directions this handle resizes in.
    pub fn cursor(&self) -> &'static str {
        match (self.x, self.y) {
            (Edge::Neither, _) => "ns-resize",
            (_, Edge::Neither) => "ew-resize",
            (Edge::Min, Edge::Min) | (Edge::Max, Edge::Max) => "nwse-resize",
            _ => "nesw-resize",
        }
    }
}

impl Rect {
    /// The rectangle spanned by two corners in any order.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
    }

    /// The rectangle drawn by dragging from `start` to `end`. `square` makes both sides as long as
    /// the longer one; `from_centre` treats `start` as the centre instead of a corner.
    pub fn from_drag(start: Point, end: Point, square: bool, from_centre: bool) -> Self {
        let (mut dx, mut dy) = (end.x - start.x, end.y - start.y);
        if square {
            let side = dx.abs().max(dy.abs());
            dx = side.copysign(dx);
            dy = side.copysign(dy);
        }
        if from_centre {
            Rect::from_corners(Point::new(start.x - dx, start.y - dy), Point::new(start.x + dx, start.y + dy))
        } else {
            Rect::from_corners(start, Point::new(start.x + dx, start.y + dy))
        }
    }

    /// The part of the rectangle inside a `width` × `height` image.
    pub fn clamped(&self, width: f64, height: f64) -> Self {
        let clamp = |value: f64, max: f64| value.clamp(0.0, max);
        Rect::new(clamp(self.min.x, width), clamp(self.min.y, height), clamp(self.max.x, width), clamp(self.max.y, height))
    }

    /// Whether `point` lies inside the rectangle grown by `margin` on every side.
    pub fn contains(&self, point: Point, margin: f64) -> bool {
        point.x >= self.min.x - margin
            && point.x <= self.max.x + margin
            && point.y >= self.min.y - margin
            && point.y <= self.max.y + margin
    }

    /// Whether the two rectangles share any area or touch.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.min.x <= other.max.x && other.min.x <= self.max.x && self.min.y <= other.max.y && other.min.y <= self.max.y
    }

    /// The smallest rectangle containing both.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect::new(
            self.min.x.min(other.min.x),
            self.min.y.min(other.min.y),
            self.max.x.max(other.max.x),
            self.max.y.max(other.max.y),
        )
    }

    pub fn centre(&self) -> Point {
        Point::new((self.min.x + self.max.x) / 2.0, (self.min.y + self.max.y) / 2.0)
    }

    /// Where `handle` is drawn.
    pub fn handle_position(&self, handle: Handle) -> Point {
        let along = |edge: Edge, min: f64, max: f64| match edge {
            Edge::Min => min,
            Edge::Max => max,
            Edge::Neither => (min + max) / 2.0,
        };
        Point::new(along(handle.x, self.min.x, self.max.x), along(handle.y, self.min.y, self.max.y))
    }

    /// The handle nearest to `point` within `tolerance` (per axis); corners win ties.
    pub fn handle_at(&self, point: Point, tolerance: f64) -> Option<Handle> {
        let offset = |handle: &Handle| {
            let at = self.handle_position(*handle);
            ((at.x - point.x).abs(), (at.y - point.y).abs())
        };
        let is_side = |handle: &Handle| handle.x == Edge::Neither || handle.y == Edge::Neither;
        Handle::ALL
            .iter()
            .filter(|handle| {
                let (dx, dy) = offset(handle);
                dx <= tolerance && dy <= tolerance
            })
            .min_by(|a, b| {
                let distance = |handle| {
                    let (dx, dy) = offset(handle);
                    dx.hypot(dy)
                };
                distance(a).total_cmp(&distance(b)).then(is_side(a).cmp(&is_side(b)))
            })
            .copied()
    }

    /// The rectangle after dragging `handle` to `to`. Dragging past the opposite edge flips it.
    pub fn resized(&self, handle: Handle, to: Point) -> Self {
        let mut corners = (self.min, self.max);
        match handle.x {
            Edge::Min => corners.0.x = to.x,
            Edge::Max => corners.1.x = to.x,
            Edge::Neither => {}
        }
        match handle.y {
            Edge::Min => corners.0.y = to.y,
            Edge::Max => corners.1.y = to.y,
            Edge::Neither => {}
        }
        Rect::from_corners(corners.0, corners.1)
    }

    /// The rectangle moved by (`dx`, `dy`) but kept whole inside a `width` × `height` image.
    pub fn moved_within(&self, dx: f64, dy: f64, width: f64, height: f64) -> Self {
        let dx = dx.clamp(-self.min.x, width - self.max.x);
        let dy = dy.clamp(-self.min.y, height - self.max.y);
        Rect::new(self.min.x + dx, self.min.y + dy, self.max.x + dx, self.max.y + dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn angles_snap_to_steps_keeping_the_length() {
        let from = Point::new(0.0, 0.0);
        // 10° is nearer to 15° than to 0°.
        let to = Point::new(10f64.to_radians().cos() * 50.0, 10f64.to_radians().sin() * 50.0);
        let snapped = snap_angle(from, to, 15.0);
        assert!(close(snapped, Point::new(15f64.to_radians().cos() * 50.0, 15f64.to_radians().sin() * 50.0)));
        assert!(close(snap_angle(from, Point::new(30.0, 2.0), 15.0), Point::new(30.0f64.hypot(2.0), 0.0)));
        assert!(close(snap_angle(from, Point::new(0.0, -20.0), 15.0), Point::new(0.0, -20.0)));
    }

    #[test]
    fn distance_to_a_segment_uses_the_nearest_point_on_it() {
        let (a, b) = (Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert_eq!(distance_to_segment(Point::new(5.0, 3.0), a, b), 3.0);
        assert_eq!(distance_to_segment(Point::new(14.0, 3.0), a, b), 5.0);
        assert_eq!(distance_to_segment(Point::new(1.0, 1.0), a, a), 2f64.sqrt());
    }

    #[test]
    fn points_inside_and_outside_a_concave_polygon() {
        // A "U": the notch between x 4..6 above y 4 is outside.
        let u = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (6.0, 10.0), (6.0, 4.0), (4.0, 4.0), (4.0, 10.0), (0.0, 10.0)]
            .map(|(x, y)| Point::new(x, y));
        assert!(inside_polygon(Point::new(2.0, 8.0), &u));
        assert!(!inside_polygon(Point::new(5.0, 8.0), &u));
        assert!(inside_polygon(Point::new(5.0, 2.0), &u));
        assert!(!inside_polygon(Point::new(11.0, 5.0), &u));
        assert_eq!(polygon_area(&u), 100.0 - 12.0);
    }

    #[test]
    fn bounds_of_points() {
        let points = [Point::new(3.0, 7.0), Point::new(-1.0, 2.0), Point::new(5.0, 4.0)];
        assert_eq!(bounds_of(&points), Rect::new(-1.0, 2.0, 5.0, 7.0));
    }

    const START: Point = Point::new(10.0, 10.0);

    #[test]
    fn dragging_in_any_direction_gives_a_normalized_rectangle() {
        assert_eq!(Rect::from_drag(START, Point::new(30.0, 20.0), false, false), Rect::new(10.0, 10.0, 30.0, 20.0));
        assert_eq!(Rect::from_drag(START, Point::new(4.0, 2.0), false, false), Rect::new(4.0, 2.0, 10.0, 10.0));
    }

    #[test]
    fn shift_makes_a_square_towards_the_cursor() {
        assert_eq!(Rect::from_drag(START, Point::new(30.0, 15.0), true, false), Rect::new(10.0, 10.0, 30.0, 30.0));
        assert_eq!(Rect::from_drag(START, Point::new(5.0, -10.0), true, false), Rect::new(-10.0, -10.0, 10.0, 10.0));
    }

    #[test]
    fn alt_draws_from_the_centre() {
        assert_eq!(Rect::from_drag(START, Point::new(14.0, 13.0), false, true), Rect::new(6.0, 7.0, 14.0, 13.0));
        assert_eq!(Rect::from_drag(START, Point::new(14.0, 12.0), true, true), Rect::new(6.0, 6.0, 14.0, 14.0));
    }

    #[test]
    fn rectangles_are_clamped_to_the_image() {
        assert_eq!(Rect::new(-5.0, 10.0, 120.0, 50.0).clamped(100.0, 40.0), Rect::new(0.0, 10.0, 100.0, 40.0));
    }

    #[test]
    fn containment_allows_a_margin() {
        let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert!(rect.contains(Point::new(15.0, 15.0), 0.0));
        assert!(!rect.contains(Point::new(22.0, 15.0), 0.0));
        assert!(rect.contains(Point::new(22.0, 15.0), 3.0));
    }

    #[test]
    fn intersection_and_union() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(a.intersects(&Rect::new(5.0, 5.0, 20.0, 20.0)));
        assert!(a.intersects(&Rect::new(10.0, 0.0, 20.0, 5.0)));
        assert!(!a.intersects(&Rect::new(11.0, 0.0, 20.0, 5.0)));
        assert_eq!(a.union(&Rect::new(5.0, -5.0, 20.0, 5.0)), Rect::new(0.0, -5.0, 20.0, 10.0));
    }

    #[test]
    fn handles_sit_on_corners_and_side_midpoints() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let positions: Vec<Point> = Handle::ALL.iter().map(|h| rect.handle_position(*h)).collect();
        assert_eq!(positions[0], Point::new(0.0, 0.0));
        assert_eq!(positions[1], Point::new(50.0, 0.0));
        assert_eq!(positions[4], Point::new(100.0, 50.0));
        assert_eq!(positions[7], Point::new(0.0, 25.0));
    }

    #[test]
    fn the_nearest_handle_under_the_pointer_is_found() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(rect.handle_at(Point::new(98.0, 52.0), 4.0), Some(Handle { x: Edge::Max, y: Edge::Max }));
        assert_eq!(rect.handle_at(Point::new(51.0, -1.0), 4.0), Some(Handle { x: Edge::Neither, y: Edge::Min }));
        assert_eq!(rect.handle_at(Point::new(50.0, 25.0), 4.0), None);
        // On a tiny rectangle every handle is in reach; the nearest one wins.
        let tiny = Rect::new(0.0, 0.0, 4.0, 4.0);
        assert_eq!(tiny.handle_at(Point::new(3.5, 0.5), 4.0), Some(Handle { x: Edge::Max, y: Edge::Min }));
        assert_eq!(tiny.handle_at(Point::new(4.0, 2.2), 4.0), Some(Handle { x: Edge::Max, y: Edge::Neither }));
    }

    #[test]
    fn resizing_moves_only_the_handle_edges_and_flips_past_the_opposite_edge() {
        let rect = Rect::new(10.0, 10.0, 50.0, 30.0);
        let right = Handle { x: Edge::Max, y: Edge::Neither };
        assert_eq!(rect.resized(right, Point::new(70.0, 99.0)), Rect::new(10.0, 10.0, 70.0, 30.0));
        assert_eq!(rect.resized(right, Point::new(0.0, 99.0)), Rect::new(0.0, 10.0, 10.0, 30.0));
        let top_left = Handle { x: Edge::Min, y: Edge::Min };
        assert_eq!(rect.resized(top_left, Point::new(5.0, 20.0)), Rect::new(5.0, 20.0, 50.0, 30.0));
    }

    #[test]
    fn moving_keeps_the_rectangle_inside_the_image() {
        let rect = Rect::new(10.0, 10.0, 30.0, 20.0);
        assert_eq!(rect.moved_within(5.0, -3.0, 100.0, 100.0), Rect::new(15.0, 7.0, 35.0, 17.0));
        assert_eq!(rect.moved_within(-50.0, 500.0, 100.0, 100.0), Rect::new(0.0, 90.0, 20.0, 100.0));
    }
}
