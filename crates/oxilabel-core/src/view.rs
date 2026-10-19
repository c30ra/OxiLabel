//! The transform between screen pixels and original-image pixels in the annotation workspace.
//! Annotations are always stored in image coordinates; only the view changes when zooming or panning.

use crate::geometry::{Point, Rect};

/// Closest and farthest zoom: 1% and 3200%.
pub const MIN_SCALE: f64 = 0.01;
pub const MAX_SCALE: f64 = 32.0;

/// `screen = image * scale + offset`, with screen in CSS pixels relative to the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub scale: f64,
    pub offset: Point,
}

impl View {
    /// The whole image centred in the viewport with `margin` screen pixels around it. Small images are
    /// not enlarged past 100%.
    pub fn fit(image: (u32, u32), viewport: (f64, f64), margin: f64) -> Self {
        let (width, height) = (f64::from(image.0.max(1)), f64::from(image.1.max(1)));
        let room = ((viewport.0 - 2.0 * margin).max(1.0), (viewport.1 - 2.0 * margin).max(1.0));
        let scale = (room.0 / width).min(room.1 / height).clamp(MIN_SCALE, 1.0);
        let offset = Point::new((viewport.0 - width * scale) / 2.0, (viewport.1 - height * scale) / 2.0);
        View { scale, offset }
    }

    pub fn to_screen(&self, image: Point) -> Point {
        Point::new(image.x * self.scale + self.offset.x, image.y * self.scale + self.offset.y)
    }

    pub fn to_image(&self, screen: Point) -> Point {
        Point::new((screen.x - self.offset.x) / self.scale, (screen.y - self.offset.y) / self.scale)
    }

    /// Multiplies the scale by `factor` (within the zoom limits) keeping the image point under
    /// `anchor` where it is on screen.
    pub fn zoom_at(&self, anchor: Point, factor: f64) -> Self {
        self.zoom_to(anchor, self.scale * factor)
    }

    /// Sets the scale (within the zoom limits) keeping the image point under `anchor` in place.
    pub fn zoom_to(&self, anchor: Point, scale: f64) -> Self {
        let scale = scale.clamp(MIN_SCALE, MAX_SCALE);
        let fixed = self.to_image(anchor);
        View { scale, offset: Point::new(anchor.x - fixed.x * scale, anchor.y - fixed.y * scale) }
    }

    /// Moves the image by a screen-space distance.
    pub fn pan(&self, dx: f64, dy: f64) -> Self {
        View { scale: self.scale, offset: Point::new(self.offset.x + dx, self.offset.y + dy) }
    }

    /// The part of the image plane that a viewport of the given size shows.
    pub fn visible(&self, viewport: (f64, f64)) -> Rect {
        let min = self.to_image(Point::new(0.0, 0.0));
        let max = self.to_image(Point::new(viewport.0, viewport.1));
        Rect { min, max }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn screen_and_image_coordinates_round_trip() {
        let view = View { scale: 0.25, offset: Point::new(40.0, -12.5) };
        let image = Point::new(1234.5, 678.25);
        assert!(close(view.to_image(view.to_screen(image)), image));
        assert_eq!(view.to_screen(Point::new(0.0, 0.0)), Point::new(40.0, -12.5));
        assert_eq!(view.to_screen(Point::new(100.0, 100.0)), Point::new(65.0, 12.5));
    }

    #[test]
    fn fitting_centres_the_image_and_keeps_the_aspect_ratio() {
        // 3000×2000 in 1000×800 with a 20 px margin: width limits, 960/3000 = 0.32.
        let view = View::fit((3000, 2000), (1000.0, 800.0), 20.0);
        assert!((view.scale - 0.32).abs() < 1e-12);
        assert!(close(view.to_screen(Point::new(0.0, 0.0)), Point::new(20.0, 80.0)));
        assert!(close(view.to_screen(Point::new(3000.0, 2000.0)), Point::new(980.0, 720.0)));
    }

    #[test]
    fn small_images_fit_at_full_size() {
        let view = View::fit((100, 50), (1000.0, 800.0), 20.0);
        assert_eq!(view.scale, 1.0);
        assert_eq!(view.offset, Point::new(450.0, 375.0));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor_fixed() {
        let view = View { scale: 0.5, offset: Point::new(10.0, 20.0) };
        let cursor = Point::new(300.0, 200.0);
        let before = view.to_image(cursor);
        let zoomed = view.zoom_at(cursor, 2.0);
        assert_eq!(zoomed.scale, 1.0);
        assert!(close(zoomed.to_image(cursor), before));
    }

    #[test]
    fn zoom_is_limited() {
        let view = View { scale: 1.0, offset: Point::new(0.0, 0.0) };
        assert_eq!(view.zoom_at(Point::new(0.0, 0.0), 1000.0).scale, MAX_SCALE);
        assert_eq!(view.zoom_at(Point::new(0.0, 0.0), 0.0001).scale, MIN_SCALE);
    }

    #[test]
    fn panning_moves_by_screen_pixels() {
        let view = View { scale: 2.0, offset: Point::new(0.0, 0.0) }.pan(30.0, -10.0);
        assert_eq!(view.to_image(Point::new(30.0, -10.0)), Point::new(0.0, 0.0));
    }

    #[test]
    fn the_visible_region_is_in_image_coordinates() {
        let view = View { scale: 0.5, offset: Point::new(100.0, 50.0) };
        assert_eq!(view.visible((600.0, 400.0)), Rect::new(-200.0, -100.0, 1000.0, 700.0));
    }
}
