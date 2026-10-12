//! Showing and painting masks. Each mask is drawn from an offscreen canvas (a sprite) holding its
//! pixels in the label colour at image resolution, scaled by the view; sprites of stored masks are
//! cached, and a brush stroke updates only the pixels it touched.
//!
//! A brush stroke with label L extends every L mask it touches (merging them into one) or else
//! becomes a new mask; the eraser removes pixels from every mask under it, of any label. A stroke
//! is one undo step. A fuzzy fill's region is added or erased the same way.

use std::collections::BTreeMap;

use leptos::prelude::*;
use rtag_core::{
    annotation::{Annotation, Shape},
    geometry::{Point, Rect},
    history::Edit,
    mask::{Bitmap, Mask, PixelRect},
    view::View,
};
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use crate::viewer::context_2d;

/// Opacity of masks: they have no outline, so they are stronger than shape fills.
const ALPHA: f64 = 0.45;
const SELECTED_ALPHA: f64 = 0.6;

/// A bitmap's pixels on a canvas the size of its area.
pub struct Sprite {
    area: PixelRect,
    canvas: HtmlCanvasElement,
}

impl Sprite {
    fn new(bitmap: &Bitmap, color: &str) -> Option<Sprite> {
        let canvas: HtmlCanvasElement = document().create_element("canvas").ok()?.unchecked_into();
        canvas.set_width(bitmap.area.width.max(1));
        canvas.set_height(bitmap.area.height.max(1));
        let sprite = Sprite { area: bitmap.area, canvas };
        sprite.copy(bitmap, bitmap.area, color);
        Some(sprite)
    }

    /// Copies `region` of `bitmap` onto the canvas: set pixels in `color`, the rest clear.
    fn copy(&self, bitmap: &Bitmap, region: PixelRect, color: &str) {
        let region = region.intersection(&self.area);
        if region.is_empty() {
            return;
        }
        let [r, g, b] = rgb(color);
        let mut rgba = vec![0u8; region.width as usize * region.height as usize * 4];
        let (width, row_width) = (bitmap.area.width as usize, region.width as usize);
        for (y, out) in (region.top..region.bottom()).zip(rgba.chunks_exact_mut(row_width * 4)) {
            let start = (y - bitmap.area.top) as usize * width + (region.left - bitmap.area.left) as usize;
            for (&pixel, out) in bitmap.pixels[start..start + row_width].iter().zip(out.as_chunks_mut::<4>().0) {
                if pixel != 0 {
                    *out = [r, g, b, 255];
                }
            }
        }
        let Ok(data) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), region.width, region.height) else { return };
        if let Some(context) = context_2d(&self.canvas) {
            let (x, y) = (region.left - self.area.left, region.top - self.area.top);
            context.put_image_data(&data, f64::from(x), f64::from(y)).ok();
        }
    }

    /// Shows the changed `region` of `bitmap`; a bitmap that grew gets a new canvas.
    fn refresh(sprite: &mut Option<Sprite>, bitmap: &Bitmap, region: PixelRect, color: &str) {
        match sprite {
            Some(current) if current.area == bitmap.area => current.copy(bitmap, region, color),
            _ => *sprite = Sprite::new(bitmap, color),
        }
    }

    /// Draws the sprite with its top-left pixel at (`left`, `top`) in the image.
    fn draw(&self, context: &CanvasRenderingContext2d, view: View, left: u32, top: u32, selected: bool) {
        self.draw_scaled(context, view, left, top, 1, selected);
    }

    /// Draws a sprite whose pixels each cover `factor` × `factor` image pixels.
    fn draw_scaled(&self, context: &CanvasRenderingContext2d, view: View, left: u32, top: u32, factor: u32, selected: bool) {
        let at = view.to_screen(Point::new(f64::from(left), f64::from(top)));
        let scale = view.scale * f64::from(factor);
        let (width, height) = (f64::from(self.area.width) * scale, f64::from(self.area.height) * scale);
        context.set_image_smoothing_enabled(false);
        context.set_global_alpha(if selected { SELECTED_ALPHA } else { ALPHA });
        context.draw_image_with_html_canvas_element_and_dw_and_dh(&self.canvas, at.x, at.y, width, height).ok();
        context.set_global_alpha(1.0);
    }
}

/// `#rrggbb` as bytes; anything else is white.
fn rgb(color: &str) -> [u8; 3] {
    let byte = |i: usize| color.get(i..i + 2).and_then(|hex| u8::from_str_radix(hex, 16).ok()).unwrap_or(255);
    [byte(1), byte(3), byte(5)]
}

/// Sprites of stored masks, found by their pixels and colour so that moving a mask (which only
/// changes its position) reuses its sprite.
#[derive(Default)]
pub struct SpriteCache {
    entries: Vec<CachedSprite>,
}

struct CachedSprite {
    mask: Mask,
    color: String,
    sprite: Sprite,
    /// Drawn since the last sweep.
    drawn: bool,
}

impl SpriteCache {
    /// Draws `mask` in `color`, making its sprite if it has none yet.
    pub fn draw(&mut self, context: &CanvasRenderingContext2d, view: View, mask: &Mask, color: &str, selected: bool) {
        let same = |cached: &CachedSprite| {
            cached.color == color && cached.mask.width == mask.width && cached.mask.height == mask.height && cached.mask.rle == mask.rle
        };
        let index = match self.entries.iter().position(same) {
            Some(index) => index,
            None => {
                let Some(sprite) = Sprite::new(&mask.to_bitmap(), color) else { return };
                self.entries.push(CachedSprite { mask: mask.clone(), color: color.to_owned(), sprite, drawn: false });
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[index];
        entry.drawn = true;
        entry.sprite.draw(context, view, mask.left, mask.top, selected);
    }

    /// Forgets the sprites not drawn since the last sweep.
    pub fn sweep(&mut self) {
        self.entries.retain(|entry| entry.drawn);
        self.entries.iter_mut().for_each(|entry| entry.drawn = false);
    }
}

/// A fuzzy fill's region before it is committed, at the resolution of the analysed level, whose
/// pixels each cover `factor` × `factor` image pixels.
pub struct FillPreview {
    pub region: Bitmap,
    pub factor: u32,
    sprite: Option<Sprite>,
}

impl FillPreview {
    pub fn new(region: Bitmap, factor: u32, color: &str) -> FillPreview {
        let sprite = Sprite::new(&region, color);
        FillPreview { region, factor, sprite }
    }

    pub fn draw(&self, context: &CanvasRenderingContext2d, view: View) {
        if let Some(sprite) = &self.sprite {
            let (left, top) = (self.region.area.left * self.factor, self.region.area.top * self.factor);
            sprite.draw_scaled(context, view, left, top, self.factor, true);
        }
    }
}

/// A brush or eraser stroke in progress.
pub struct Stroke {
    erase: bool,
    label: i64,
    radius: f64,
    image: (u32, u32),
    pub last: Point,
    /// Brush: the stroke's pixels and those of the masks it joined.
    painted: Bitmap,
    sprite: Option<Sprite>,
    color: String,
    /// Brush: the annotations merged into `painted` (no longer drawn on their own).
    merged: Vec<usize>,
    /// Brush: decoded masks with the stroke's label, to check whether the stroke touches them.
    candidates: BTreeMap<usize, Bitmap>,
    /// Eraser: working copies of the masks it has touched, by annotation index.
    erased: BTreeMap<usize, (Bitmap, Option<Sprite>, String)>,
}

impl Stroke {
    /// Starts a stroke at `at` and applies its first dab.
    pub fn start(erase: bool, label: i64, size: f64, image: (u32, u32), at: Point, annotations: &[Annotation], colors: &dyn Fn(i64) -> String) -> Stroke {
        let mut stroke = Stroke { radius: size / 2.0, last: at, color: colors(label), ..Stroke::empty(erase, label, image) };
        stroke.extend(at, annotations, colors);
        stroke
    }

    fn empty(erase: bool, label: i64, image: (u32, u32)) -> Stroke {
        Stroke {
            erase,
            label,
            radius: 0.0,
            image,
            last: Point::new(0.0, 0.0),
            painted: Bitmap::default(),
            sprite: None,
            color: String::new(),
            merged: Vec::new(),
            candidates: BTreeMap::new(),
            erased: BTreeMap::new(),
        }
    }

    /// A finished stroke covering `region` (image pixels), e.g. a fuzzy fill.
    pub fn from_region(erase: bool, label: i64, region: Bitmap, image: (u32, u32), annotations: &[Annotation]) -> Stroke {
        let mut stroke = Stroke::empty(erase, label, image);
        let Some(bounds) = region.bounds() else { return stroke };
        for (index, annotation) in annotations.iter().enumerate() {
            let Shape::Mask(mask) = &annotation.shape else { continue };
            if mask.area().intersection(&bounds).is_empty() {
                continue;
            }
            let mut decoded = mask.to_bitmap();
            if erase {
                decoded.subtract(&region);
                stroke.erased.insert(index, (decoded, None, String::new()));
            } else if annotation.label == label && region.overlaps(&decoded, bounds) {
                stroke.painted.merge(&decoded, image);
                stroke.merged.push(index);
            }
        }
        stroke.painted.merge(&region, image);
        stroke
    }

    /// Drags the brush from the last position to `to`.
    pub fn extend(&mut self, to: Point, annotations: &[Annotation], colors: &dyn Fn(i64) -> String) {
        let from = self.last;
        self.last = to;
        if self.erase {
            self.erase_along(from, to, annotations, colors);
        } else {
            self.paint_along(from, to, annotations);
        }
    }

    fn paint_along(&mut self, from: Point, to: Point, annotations: &[Annotation]) {
        let covered = self.painted.paint(from, to, self.radius, self.image);
        if covered.is_empty() {
            return;
        }
        // Only the brush's own pixels join masks, not those of masks merged before.
        let mut dab = Bitmap::default();
        dab.paint(from, to, self.radius, self.image);
        let area_before = self.painted.area;
        for (index, annotation) in annotations.iter().enumerate() {
            let Shape::Mask(mask) = &annotation.shape else { continue };
            if annotation.label != self.label || self.merged.contains(&index) || mask.area().intersection(&covered).is_empty() {
                continue;
            }
            let decoded = self.candidates.entry(index).or_insert_with(|| mask.to_bitmap());
            if dab.overlaps(decoded, covered) {
                self.painted.merge(decoded, self.image);
                self.merged.push(index);
                self.candidates.remove(&index);
                // Its pixels may lie anywhere in the sprite: copy everything again.
                self.sprite = None;
            }
        }
        let region = if self.painted.area == area_before { covered } else { self.painted.area };
        Sprite::refresh(&mut self.sprite, &self.painted, region, &self.color);
    }

    fn erase_along(&mut self, from: Point, to: Point, annotations: &[Annotation], colors: &dyn Fn(i64) -> String) {
        let reach = Rect::from_corners(from, to);
        let reach = Rect::new(reach.min.x - self.radius, reach.min.y - self.radius, reach.max.x + self.radius, reach.max.y + self.radius);
        for (index, annotation) in annotations.iter().enumerate() {
            let Shape::Mask(mask) = &annotation.shape else { continue };
            if !annotation.shape.bounds().intersects(&reach) {
                continue;
            }
            let (bitmap, sprite, color) = self.erased.entry(index).or_insert_with(|| {
                let color = colors(annotation.label);
                let bitmap = mask.to_bitmap();
                let sprite = Sprite::new(&bitmap, &color);
                (bitmap, sprite, color)
            });
            let touched = bitmap.erase(from, to, self.radius);
            Sprite::refresh(sprite, bitmap, touched, color);
        }
    }
}

impl Stroke {
    /// Draws annotation `index` as the stroke has changed it so far: as the eraser left it, or
    /// not at all if it joined the brush stroke. False if the stroke has not changed it.
    pub fn draw_changed(&self, context: &CanvasRenderingContext2d, view: View, index: usize, selected: bool) -> bool {
        if let Some((bitmap, sprite, _)) = self.erased.get(&index) {
            if let Some(sprite) = sprite {
                sprite.draw(context, view, bitmap.area.left, bitmap.area.top, selected);
            }
            return true;
        }
        self.merged.contains(&index)
    }

    /// Draws the brush stroke (with the masks it joined).
    pub fn draw_painted(&self, context: &CanvasRenderingContext2d, view: View) {
        if let Some(sprite) = &self.sprite {
            sprite.draw(context, view, self.painted.area.left, self.painted.area.top, false);
        }
    }

    /// The edit that makes the stroke permanent, if it changed anything: a new mask, or the
    /// joined masks replaced by their union (at the place of the lowest one); for the eraser the
    /// changed masks, and those left empty removed.
    pub fn finish(self, annotations: &[Annotation]) -> Option<Edit> {
        let mut changes = Vec::new();
        let mut removals = Vec::new();
        if self.erase {
            for (index, (bitmap, ..)) in self.erased {
                let Some(before) = annotations.get(index).cloned() else { continue };
                match bitmap.to_mask().map(Shape::Mask) {
                    Some(shape) if shape != before.shape => {
                        changes.push(Edit::Replace { index, after: Annotation { shape, ..before.clone() }, before })
                    }
                    Some(_) => {}
                    None => removals.push(Edit::Remove { index, annotation: before }),
                }
            }
        } else {
            let shape = Shape::Mask(self.painted.to_mask()?);
            let mut merged = self.merged;
            merged.sort_unstable();
            match merged.split_first() {
                None => {
                    let annotation = Annotation::manual(self.label, shape);
                    changes.push(Edit::Add { index: annotations.len(), annotation });
                }
                Some((&first, others)) => {
                    let before = annotations.get(first)?.clone();
                    changes.push(Edit::Replace { index: first, after: Annotation { shape, ..before.clone() }, before });
                    for &index in others {
                        if let Some(annotation) = annotations.get(index) {
                            removals.push(Edit::Remove { index, annotation: annotation.clone() });
                        }
                    }
                }
            }
        }
        // Highest index first, so the other removals keep their positions.
        removals.reverse();
        changes.extend(removals);
        match changes.len() {
            0 => None,
            1 => changes.pop(),
            _ => Some(Edit::Batch(changes)),
        }
    }
}
