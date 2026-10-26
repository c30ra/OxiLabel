//! Analysis tiles for the fuzzy fill (ADR 0002): the image's lossless PNG pyramid, fetched tile by
//! tile as a fill reaches them, and read back as pixels through a canvas. Each tool analyses the
//! original or, with its switch on, the active enhancement filter's pyramid: the `source` is that
//! filter's key, `None` for the original.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use oxilabel_core::{
    api::{TileQuery, with_query},
    fill::{Raster, Tiles},
};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{HtmlCanvasElement, HtmlImageElement};

use crate::viewer::context_2d;

type Source = Option<String>;

#[derive(Default)]
struct Store {
    levels: HashMap<(Source, u32), Tiles>,
    requested: HashSet<(Source, u32, u32, u32)>,
}

/// The analysis tiles of one image loaded so far. `loaded` changes whenever one arrives, and
/// `failed` when one cannot be read.
#[derive(Clone, Copy)]
pub struct Analysis {
    image: i64,
    store: StoredValue<Store, LocalStorage>,
    pub loaded: RwSignal<u32>,
    pub failed: RwSignal<bool>,
}

impl Analysis {
    pub fn new(image: i64) -> Self {
        Analysis { image, store: StoredValue::new_local(Store::default()), loaded: RwSignal::new(0), failed: RwSignal::new(false) }
    }

    /// Calls `with` on the loaded tiles of `level` of `source`.
    pub fn with_level<R>(&self, source: &Source, level: u32, with: impl FnOnce(&Tiles) -> R) -> R {
        let empty = Tiles::new();
        self.store.with_value(|store| with(store.levels.get(&(source.clone(), level)).unwrap_or(&empty)))
    }

    /// Starts loading those of the (column, row) `tiles` of `level` of `source` not asked for before.
    pub fn request(&self, source: &Source, level: u32, tiles: &[(u32, u32)]) {
        for &(column, row) in tiles {
            let new = self.store.try_update_value(|store| store.requested.insert((source.clone(), level, column, row))).unwrap_or(false);
            if new {
                // Only the original's and this source's pixels are kept; other filters' go.
                self.store.update_value(|store| {
                    store.levels.retain(|(kept, _), _| kept.is_none() || kept == source);
                    store.requested.retain(|(kept, ..)| kept.is_none() || kept == source);
                    store.requested.insert((source.clone(), level, column, row));
                });
                self.load(source.clone(), level, column, row);
            }
        }
    }

    fn load(&self, source: Source, level: u32, column: u32, row: u32) {
        let Ok(element) = HtmlImageElement::new() else { return };
        let this = *self;
        let image = element.clone();
        let url = with_query(&format!("/api/images/{}/analysis/{level}/{column}/{row}", self.image), &TileQuery { filter: source.clone() }.to_query());
        let failed = source.clone();
        let on_load = Closure::once_into_js(move || match read_pixels(&image) {
            Some(raster) => {
                this.store.update_value(|store| {
                    store.levels.entry((source, level)).or_default().insert((column, row), raster);
                });
                this.loaded.update(|n| *n = n.wrapping_add(1));
            }
            None => this.fail(source, level, column, row),
        });
        let on_error = Closure::once_into_js(move || this.fail(failed, level, column, row));
        element.set_onload(Some(on_load.unchecked_ref()));
        element.set_onerror(Some(on_error.unchecked_ref()));
        element.set_src(&url);
    }

    /// Reports a tile that could not be read, and forgets asking for it so a later fill retries.
    fn fail(&self, source: Source, level: u32, column: u32, row: u32) {
        self.store.update_value(|store| {
            store.requested.remove(&(source, level, column, row));
        });
        self.failed.set(true);
    }
}

/// The RGBA pixels of a loaded image element.
fn read_pixels(image: &HtmlImageElement) -> Option<Raster> {
    let (width, height) = (image.natural_width(), image.natural_height());
    let canvas: HtmlCanvasElement = document().create_element("canvas").ok()?.unchecked_into();
    canvas.set_width(width);
    canvas.set_height(height);
    let context = context_2d(&canvas)?;
    context.draw_image_with_html_image_element(image, 0.0, 0.0).ok()?;
    let data = context.get_image_data(0.0, 0.0, f64::from(width), f64::from(height)).ok()?;
    Some(Raster { width, height, channels: 4, pixels: data.data().0 })
}
