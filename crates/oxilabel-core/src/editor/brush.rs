//! The brush, its eraser and the fuzzy fill: painting and erasing masks with a core
//! [`MaskStroke`], the F gesture that changes the brush's size, and a fill seeded by a press,
//! grown again by the threshold a drag sets, and committed once the button is released and
//! growing no longer reaches unloaded tiles.
//!
//! The stroke's pixels are the image's, so the annotation layer supplies them: [`Layer::fill`]
//! grows a region, [`Layer::limit`] is "Stop at edges". The layer draws what the session keeps:
//! the stroke's pixels and the fill's region.

use std::mem;

use crate::{
    ShapeType,
    attribute::Attributes,
    geometry::Point,
    history::Edit,
    mask::Bitmap,
    stroke::{MaskStroke, SegmentChange, StrokeOutcome},
};

use super::{
    BRUSH_STEP, EdgePath, Gesture, MAX_BRUSH_SIZE, MAX_THRESHOLD, MIN_BRUSH_SIZE, Pointer, Session,
    THRESHOLD_PER_PIXEL, THRESHOLD_STEP, Tool,
};

/// Grows the fuzzy fill's region from `seed` (image pixels) at `threshold`, on the analysed tiles
/// loaded so far.
pub type FillSource<'a> = &'a dyn Fn(Point, f64) -> FillRegion;
/// The pixels a "Stop at edges" brush starting at `seed` may paint, in image pixels.
pub type LimitSource<'a> = &'a dyn Fn(Point) -> Bitmap;

/// What the annotation layer supplies to gestures that need the image's pixels: the path a
/// freehand polyline follows, the region a fuzzy fill grows on, and the pixels a "Stop at edges"
/// brush may paint. Every part is optional: a gesture without it behaves as if there were none.
#[derive(Clone, Copy, Default)]
pub struct Layer<'a> {
    pub edges: Option<EdgePath<'a>>,
    pub fill: Option<FillSource<'a>>,
    pub limit: Option<LimitSource<'a>>,
}

/// A fuzzy fill's region as its gesture has grown it: `factor` × `factor` image pixels per pixel
/// of the level it was grown on, `incomplete` while it reaches tiles that are not loaded (it then
/// grows again as they arrive) and `capped` when it stopped at the maximum size.
#[derive(Debug, Clone, PartialEq)]
pub struct FillRegion {
    pub region: Bitmap,
    pub factor: u32,
    pub incomplete: bool,
    pub capped: bool,
}

impl Session {
    /// The brush's diameter in image pixels.
    pub fn brush_size(&self) -> f64 {
        self.options.brush_size
    }

    /// The fuzzy fill's threshold (0–255).
    pub fn fill_threshold(&self) -> f64 {
        self.options.threshold
    }

    /// Whether a brush stroke, an eraser stroke or a fuzzy fill is in progress.
    pub fn is_brushing(&self) -> bool {
        matches!(self.gesture, Gesture::Painting { .. } | Gesture::Filling { .. })
    }

    /// Whether the brush's size is being changed with F.
    pub fn is_sizing_brush(&self) -> bool {
        matches!(self.gesture, Gesture::BrushSizing { .. })
    }

    /// Where the brush outline is while its size is changed with F, and the size.
    pub fn brush_sizing(&self) -> Option<(Point, f64)> {
        match &self.gesture {
            Gesture::BrushSizing { centre, .. } => Some((*centre, self.options.brush_size)),
            _ => None,
        }
    }

    /// Starts changing the brush's size around `centre` (F where the pointer is); false when the
    /// brush is not painting or something else is in progress.
    pub fn begin_brush_sizing(&mut self, centre: Point) -> bool {
        if self.tool != Tool::Brush || self.brush.fills() || !matches!(self.gesture, Gesture::Idle) {
            return false;
        }
        self.gesture = Gesture::BrushSizing { centre: self.inside(centre), before: self.options.brush_size };
        true
    }

    /// The brush stroke's painted pixels (with the masks they joined), while it paints.
    pub fn stroke_painted(&self) -> Option<&Bitmap> {
        match &self.gesture {
            Gesture::Painting { stroke, .. } if !stroke.is_erasing() => Some(stroke.painted()),
            _ => None,
        }
    }

    /// Annotation `index` as the eraser stroke has left it, if it touched it.
    pub fn stroke_erased(&self, index: usize) -> Option<&Bitmap> {
        match &self.gesture {
            Gesture::Painting { stroke, .. } => stroke.erased(index),
            _ => None,
        }
    }

    /// Whether annotation `index` joined the brush stroke (and is drawn with it).
    pub fn stroke_is_merged(&self, index: usize) -> bool {
        match &self.gesture {
            Gesture::Painting { stroke, .. } => stroke.is_merged(index),
            _ => false,
        }
    }

    /// The pixels the brush or eraser changed in its last press or move, for the layer's sprites;
    /// taking them clears them. None once the stroke has ended (or a fill was started).
    pub fn take_stroke_change(&mut self) -> Option<SegmentChange> {
        let change = self.stroke_change.take();
        matches!(self.gesture, Gesture::Painting { .. }).then_some(change).flatten()
    }

    /// The fuzzy fill's region so far, for its preview.
    pub fn fill_preview(&self) -> Option<&FillRegion> {
        match &self.gesture {
            Gesture::Filling { region: Some(region), .. } => Some(region),
            _ => None,
        }
    }

    /// Whether the fill in progress cuts pixels out of the masks.
    pub fn fill_erases(&self) -> bool {
        matches!(self.gesture, Gesture::Filling { erase: true, .. })
    }

    /// Grows the fill again, or the "Stop at edges" limit, with the layer's pixels once they have
    /// arrived or the view has changed; a released fill whose region is complete is committed.
    pub fn pixels_ready(&mut self, layer: Layer<'_>) {
        let seed = match &self.gesture {
            Gesture::Painting { seed, limited: true, .. } => Some(*seed),
            _ => None,
        };
        if let (Some(seed), Some(limit)) = (seed, layer.limit) {
            let limit = limit(seed);
            if let Gesture::Painting { stroke, .. } = &mut self.gesture {
                stroke.set_limit(limit);
            }
        }
        if matches!(self.gesture, Gesture::Filling { .. }) {
            self.grow(layer);
            self.commit_fill_if_ready();
        }
    }

    /// A press of the main button with the brush: starts a stroke, or a fuzzy fill.
    pub(super) fn brush_press(&mut self, pointer: Pointer, layer: Layer<'_>) {
        // A click while sizing the brush confirms the size.
        if self.is_sizing_brush() {
            self.gesture = Gesture::Idle;
            return;
        }
        // Ctrl inverts a stroke: the brush erases, the eraser paints.
        let erasing = self.brush.erases() != pointer.modifiers.ctrl;
        if self.label.is_none() && !erasing {
            self.notice = Some("Add a label that allows brush shapes on the Labels tab first.".into());
            return;
        }
        if !erasing && !self.label_allows_mask() {
            let name = self.label_definition().map(|label| label.name.clone()).unwrap_or_default();
            self.notice = Some(format!("“{name}” cannot be used for masks."));
            return;
        }
        let at = self.inside(pointer.at);
        if self.brush.fills() {
            self.gesture = Gesture::Filling {
                seed: at,
                anchor: pointer.at,
                base: self.options.threshold,
                erase: erasing,
                released: false,
                region: None,
            };
            self.grow(layer);
            self.commit_fill_if_ready();
            return;
        }
        let label = self.label.unwrap_or_default();
        let mut stroke = MaskStroke::begin(erasing, label, self.options.brush_size, self.image, at);
        let limited = self.options.stop_at_edges != pointer.modifiers.shift;
        stroke.limit(limited);
        if let (true, Some(limit)) = (limited, layer.limit) {
            stroke.set_limit(limit(at));
        }
        self.stroke_change = Some(stroke.segment(at, &self.annotations));
        self.gesture = Gesture::Painting { stroke, seed: at, limited };
    }

    /// The pointer moved with the brush: its size, the fill's threshold, or the stroke.
    pub(super) fn brush_move_to(&mut self, pointer: Pointer, layer: Layer<'_>) {
        match &self.gesture {
            Gesture::BrushSizing { centre, before } => {
                let size = before + 2.0 * (pointer.at.x - centre.x);
                self.options.brush_size = size.round().clamp(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE);
            }
            Gesture::Filling { released: false, .. } => {
                let (anchor, base) = match &self.gesture {
                    Gesture::Filling { anchor, base, .. } => (*anchor, *base),
                    _ => unreachable!(),
                };
                let moved = ((pointer.at.x - anchor.x) + (pointer.at.y - anchor.y)) * pointer.scale;
                self.options.threshold = (base + moved * THRESHOLD_PER_PIXEL).round().clamp(0.0, MAX_THRESHOLD);
                self.grow(layer);
            }
            // Released and waiting for tiles: the pointer no longer changes it.
            Gesture::Filling { released: true, .. } => {}
            Gesture::Painting { limited: was, .. } => {
                let to = self.inside(pointer.at);
                let limited = self.options.stop_at_edges != pointer.modifiers.shift;
                let seed = match &self.gesture {
                    Gesture::Painting { seed, .. } => *seed,
                    _ => unreachable!(),
                };
                // Turning the limit on mid-stroke grows it once; it is grown again when the view's
                // level changes or tiles arrive (see `pixels_ready`).
                let limit = (limited && !was).then(|| layer.limit.map(|limit| limit(seed))).flatten();
                let Gesture::Painting { stroke, limited: was, .. } = &mut self.gesture else { unreachable!() };
                *was = limited;
                stroke.limit(limited);
                if let Some(limit) = limit {
                    stroke.set_limit(limit);
                }
                let change = stroke.segment(to, &self.annotations);
                self.stroke_change = Some(change);
            }
            _ => {}
        }
    }

    /// The main button was released: the stroke is applied, or the fill waits for its tiles.
    pub(super) fn brush_release(&mut self) {
        if matches!(self.gesture, Gesture::Painting { .. }) {
            let Gesture::Painting { stroke, .. } = mem::replace(&mut self.gesture, Gesture::Idle) else { unreachable!() };
            if let Some(outcome) = stroke.finish(&self.annotations) {
                self.commit_stroke(outcome);
            }
        } else if matches!(self.gesture, Gesture::Filling { released: false, .. }) {
            if let Gesture::Filling { released, .. } = &mut self.gesture {
                *released = true;
            }
            self.commit_fill_if_ready();
        }
    }

    /// Grows the fill's region with the layer's pixels, and says so when it first grows past the
    /// maximum size.
    fn grow(&mut self, layer: Layer<'_>) {
        let Some(grow) = layer.fill else { return };
        let threshold = self.options.threshold;
        let Gesture::Filling { seed, region, .. } = &mut self.gesture else { return };
        let grown = grow(*seed, threshold);
        if grown.capped && !region.as_ref().is_some_and(|region| region.capped) {
            self.notice = Some("The fill grows past the maximum size and will not be added.".into());
        }
        *region = Some(grown);
    }

    /// Commits a released fill whose region no longer reaches unloaded tiles.
    fn commit_fill_if_ready(&mut self) {
        let ready = matches!(
            &self.gesture,
            Gesture::Filling { released: true, region: Some(region), .. } if !region.incomplete
        );
        if ready {
            self.commit_fill();
        }
    }

    /// Adds the fill's region to the masks (or erases it from them) as one undo step.
    fn commit_fill(&mut self) {
        let Gesture::Filling { erase, region: Some(region), .. } = mem::replace(&mut self.gesture, Gesture::Idle) else {
            return;
        };
        self.stroke_change = None;
        // A runaway fill is not added: it would be a cut-off part of a much larger region.
        if region.capped {
            self.notice = Some("Not added: the fill grew past the maximum size. Lower the threshold or raise Max.".into());
            return;
        }
        let label = self.label.unwrap_or_default();
        let pixels = region.region.scaled_up(region.factor, self.image);
        let outcome = MaskStroke::from_region(erase, label, pixels, self.image, &self.annotations).finish(&self.annotations);
        if let Some(outcome) = outcome {
            self.commit_stroke(outcome);
        }
    }

    /// Applies a finished stroke, keeping the selection on the annotations that survive it.
    fn commit_stroke(&mut self, outcome: StrokeOutcome) {
        self.selection = outcome.selection_after(&self.selection);
        let edit = self.with_defaults(outcome.edit);
        self.apply(edit);
    }

    /// `edit` with new annotations given their label's default attribute values.
    fn with_defaults(&self, edit: Edit) -> Edit {
        match edit {
            Edit::Add { index, mut annotation } => {
                if annotation.attributes.is_empty() {
                    annotation.attributes = self.default_attributes(annotation.label);
                }
                Edit::Add { index, annotation }
            }
            Edit::Batch(edits) => Edit::Batch(edits.into_iter().map(|edit| self.with_defaults(edit)).collect()),
            other => other,
        }
    }

    fn default_attributes(&self, label: i64) -> Attributes {
        self.labels.iter().find(|definition| definition.id == label).map(|definition| definition.conform_attributes(&Attributes::new())).unwrap_or_default()
    }

    /// Whether the current label allows masks.
    fn label_allows_mask(&self) -> bool {
        self.label_definition().is_some_and(|label| label.shape_types.contains(&ShapeType::Mask))
    }

    /// `[` and `]`: the fill's threshold or the brush's size in steps of 25%, but at least one
    /// pixel; false with another tool.
    pub(super) fn change_brush(&mut self, larger: bool) -> bool {
        if self.tool != Tool::Brush {
            return false;
        }
        if self.brush.fills() {
            let step = if larger { THRESHOLD_STEP } else { -THRESHOLD_STEP };
            self.options.threshold = (self.options.threshold + step).clamp(0.0, MAX_THRESHOLD);
        } else {
            let size = self.options.brush_size;
            let next = if larger { (size * BRUSH_STEP).max(size + 1.0) } else { (size / BRUSH_STEP).min(size - 1.0) };
            self.options.brush_size = next.round().clamp(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE);
        }
        true
    }

    /// F or Enter while sizing the brush: keeps the size.
    pub(super) fn confirm_brush_sizing(&mut self) -> bool {
        if !self.is_sizing_brush() {
            return false;
        }
        self.gesture = Gesture::Idle;
        true
    }

    /// Esc while sizing the brush: restores the size it had.
    pub(super) fn restore_brush_size(&mut self) {
        if let Gesture::BrushSizing { before, .. } = mem::replace(&mut self.gesture, Gesture::Idle) {
            self.options.brush_size = before;
        }
    }
}
