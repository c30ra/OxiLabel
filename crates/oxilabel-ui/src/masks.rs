//! Showing and painting masks. Each mask is drawn from an offscreen canvas (a sprite) holding its
//! pixels in the label colour at image resolution, scaled by the view; sprites of stored masks are
//! cached, and a brush stroke updates only the pixels it touched.
//!
//! The stroke's mask logic lives in `oxilabel_core::stroke`; this only draws it.

use std::collections::BTreeMap;

use leptos::prelude::*;
use oxilabel_core::{
    annotation::Annotation,
    geometry::Point,
    mask::{Bitmap, Mask, PixelRect},
    stroke::MaskStroke,
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

/// A brush or eraser stroke in progress: the core stroke and the sprites showing it.
pub struct Stroke {
    pub core: MaskStroke,
    /// Brush: the painted pixels (with the masks they joined).
    sprite: Option<Sprite>,
    color: String,
    /// Eraser: sprites of the masks it has touched, by annotation index, and their colours.
    erased: BTreeMap<usize, (Option<Sprite>, String)>,
}

impl Stroke {
    /// A stroke at `at` in `color`; its first dab comes with `extend(at, …)`.
    pub fn start(core: MaskStroke, color: String) -> Stroke {
        Stroke { core, sprite: None, color, erased: BTreeMap::new() }
    }

    /// Drags the brush to `to` and updates the sprites it changed.
    pub fn extend(&mut self, to: Point, annotations: &[Annotation], colors: &dyn Fn(i64) -> String) {
        let change = self.core.segment(to, annotations);
        if let Some(region) = change.painted {
            Sprite::refresh(&mut self.sprite, self.core.painted(), region, &self.color);
        }
        for (index, region) in change.erased {
            let Some(bitmap) = self.core.erased(index) else { continue };
            match self.erased.get_mut(&index) {
                Some((sprite, color)) => Sprite::refresh(sprite, bitmap, region, color),
                None => {
                    let color = annotations.get(index).map(|a| colors(a.label)).unwrap_or_default();
                    self.erased.insert(index, (Sprite::new(bitmap, &color), color));
                }
            }
        }
    }

    /// Draws annotation `index` as the stroke has changed it so far: as the eraser left it, or
    /// not at all if it joined the brush stroke. False if the stroke has not changed it.
    pub fn draw_changed(&self, context: &CanvasRenderingContext2d, view: View, index: usize, selected: bool) -> bool {
        if let Some((sprite, _)) = self.erased.get(&index) {
            if let (Some(sprite), Some(bitmap)) = (sprite, self.core.erased(index)) {
                sprite.draw(context, view, bitmap.area.left, bitmap.area.top, selected);
            }
            return true;
        }
        self.core.is_merged(index)
    }

    /// Draws the brush stroke (with the masks it joined).
    pub fn draw_painted(&self, context: &CanvasRenderingContext2d, view: View) {
        if let Some(sprite) = &self.sprite {
            let area = self.core.painted().area;
            sprite.draw(context, view, area.left, area.top, false);
        }
    }
}
