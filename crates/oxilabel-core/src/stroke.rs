//! A brush, eraser or fuzzy fill stroke on an image's masks, in image pixels.
//!
//! A brush stroke with label L extends every L mask it touches (merging them into one, at the
//! place of the lowest) or else becomes a new mask; the eraser removes pixels from every mask
//! under it, of any label, and removes the masks it empties. Locked and hidden masks are left
//! alone. "Stop at edges" keeps the brush inside a limit while it is on. A stroke is one edit.
//!
//! Drive it with [`MaskStroke::begin`], [`MaskStroke::segment`] per pointer move and
//! [`MaskStroke::finish`]; a fuzzy fill's region is [`MaskStroke::from_region`] then `finish`.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    annotation::{Annotation, Shape},
    geometry::Point,
    history::Edit,
    mask::{Bitmap, PixelRect},
};

/// A brush or eraser stroke in progress.
#[derive(Debug, Clone)]
pub struct MaskStroke {
    erase: bool,
    label: i64,
    radius: f64,
    image: (u32, u32),
    last: Point,
    /// Brush: the stroke's pixels and those of the masks it joined.
    painted: Bitmap,
    /// Brush: the annotations merged into `painted`.
    merged: Vec<usize>,
    /// Brush: decoded masks with the stroke's label, to check whether the stroke touches them.
    candidates: BTreeMap<usize, Bitmap>,
    /// Eraser: working copies of the masks it has touched, by annotation index.
    erased: BTreeMap<usize, Bitmap>,
    /// "Stop at edges": the brush only reaches these pixels while `limited`.
    limit: Option<Bitmap>,
    limited: bool,
}

/// What one segment changed, for redrawing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SegmentChange {
    /// Brush: the part of [`MaskStroke::painted`] that changed.
    pub painted: Option<PixelRect>,
    /// Eraser: the masks it changed (see [`MaskStroke::erased`]) and the part of each.
    pub erased: Vec<(usize, PixelRect)>,
}

/// A finished stroke: its edit, and the indices (before the edit) of the annotations it removes,
/// highest first.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeOutcome {
    pub edit: Edit,
    pub removed: Vec<usize>,
}

impl StrokeOutcome {
    /// `selection` after the edit: the removed annotations dropped, the later ones shifted down.
    pub fn selection_after(&self, selection: &BTreeSet<usize>) -> BTreeSet<usize> {
        selection
            .iter()
            .filter(|index| !self.removed.contains(index))
            .map(|&index| index - self.removed.iter().filter(|&&removed| removed < index).count())
            .collect()
    }
}

impl MaskStroke {
    /// A stroke of brush diameter `size` at `at`; its first dab comes with `segment(at, …)`.
    pub fn begin(erase: bool, label: i64, size: f64, image: (u32, u32), at: Point) -> MaskStroke {
        MaskStroke { radius: size / 2.0, last: at, ..MaskStroke::empty(erase, label, image) }
    }

    fn empty(erase: bool, label: i64, image: (u32, u32)) -> MaskStroke {
        MaskStroke {
            erase,
            label,
            radius: 0.0,
            image,
            last: Point::new(0.0, 0.0),
            painted: Bitmap::default(),
            merged: Vec::new(),
            candidates: BTreeMap::new(),
            erased: BTreeMap::new(),
            limit: None,
            limited: false,
        }
    }

    /// A finished stroke covering `region` (image pixels), e.g. a fuzzy fill.
    pub fn from_region(erase: bool, label: i64, region: Bitmap, image: (u32, u32), annotations: &[Annotation]) -> MaskStroke {
        let mut stroke = MaskStroke::empty(erase, label, image);
        let Some(bounds) = region.bounds() else { return stroke };
        for (index, annotation) in annotations.iter().enumerate() {
            let Some(mask) = editable_mask(annotation) else { continue };
            if mask.area().intersection(&bounds).is_empty() {
                continue;
            }
            let mut decoded = mask.to_bitmap();
            if erase {
                decoded.subtract(&region);
                stroke.erased.insert(index, decoded);
            } else if annotation.label == label && region.overlaps(&decoded, bounds) {
                stroke.painted.merge(&decoded, image);
                stroke.merged.push(index);
            }
        }
        if !erase {
            stroke.painted.merge(&region, image);
        }
        stroke
    }

    pub fn is_erasing(&self) -> bool {
        self.erase
    }

    /// Keeps the rest of the stroke inside `limit` (image pixels) while limited; what is
    /// painted stays.
    pub fn set_limit(&mut self, limit: Bitmap) {
        self.limit = Some(limit);
    }

    /// Turns the limit on or off (Shift inverts "Stop at edges" mid-stroke).
    pub fn limit(&mut self, on: bool) {
        self.limited = on;
    }

    /// Brush: the stroke's pixels with the masks it joined.
    pub fn painted(&self) -> &Bitmap {
        &self.painted
    }

    /// Eraser: annotation `index` as the stroke has left it, if it touched it.
    pub fn erased(&self, index: usize) -> Option<&Bitmap> {
        self.erased.get(&index)
    }

    /// Brush: whether annotation `index` joined the stroke (and is drawn with it).
    pub fn is_merged(&self, index: usize) -> bool {
        self.merged.contains(&index)
    }

    /// Drags the brush from the last position to `to`.
    pub fn segment(&mut self, to: Point, annotations: &[Annotation]) -> SegmentChange {
        let from = self.last;
        self.last = to;
        if self.erase { self.erase_along(from, to, annotations) } else { self.paint_along(from, to, annotations) }
    }

    /// The pixels the brush covers from `from` to `to`, inside the limit if it is on.
    fn dab(&self, from: Point, to: Point) -> (Bitmap, PixelRect) {
        let mut dab = Bitmap::default();
        let covered = dab.paint(from, to, self.radius, self.image);
        if let (true, Some(limit)) = (self.limited, &self.limit) {
            dab.keep_only(limit);
        }
        (dab, covered)
    }

    fn paint_along(&mut self, from: Point, to: Point, annotations: &[Annotation]) -> SegmentChange {
        // Only the brush's own pixels join masks, not those of masks merged before.
        let (dab, covered) = self.dab(from, to);
        if covered.is_empty() {
            return SegmentChange::default();
        }
        let area_before = self.painted.area;
        self.painted.merge(&dab, self.image);
        let mut joined = false;
        for (index, annotation) in annotations.iter().enumerate() {
            let Some(mask) = editable_mask(annotation) else { continue };
            if annotation.label != self.label || self.merged.contains(&index) || mask.area().intersection(&covered).is_empty() {
                continue;
            }
            let decoded = self.candidates.entry(index).or_insert_with(|| mask.to_bitmap());
            if dab.overlaps(decoded, covered) {
                self.painted.merge(decoded, self.image);
                self.merged.push(index);
                self.candidates.remove(&index);
                joined = true;
            }
        }
        // A joined mask's pixels may lie anywhere in the bitmap.
        let region = if joined || self.painted.area != area_before { self.painted.area } else { covered };
        SegmentChange { painted: Some(region), erased: Vec::new() }
    }

    fn erase_along(&mut self, from: Point, to: Point, annotations: &[Annotation]) -> SegmentChange {
        let (dab, covered) = self.dab(from, to);
        let mut erased = Vec::new();
        for (index, annotation) in annotations.iter().enumerate() {
            let Some(mask) = editable_mask(annotation) else { continue };
            if mask.area().intersection(&covered).is_empty() {
                continue;
            }
            let bitmap = self.erased.entry(index).or_insert_with(|| mask.to_bitmap());
            erased.push((index, bitmap.subtract(&dab)));
        }
        SegmentChange { painted: None, erased }
    }

    /// The edit that makes the stroke permanent, if it changed anything: a new mask, or the
    /// joined masks replaced by their union (at the place of the lowest one); for the eraser the
    /// changed masks, and those left empty removed. Removals come last, highest index first, so
    /// each index is still valid when applied.
    pub fn finish(self, annotations: &[Annotation]) -> Option<StrokeOutcome> {
        let mut changes = Vec::new();
        let mut removed = Vec::new();
        if self.erase {
            for (index, bitmap) in self.erased {
                let Some(before) = annotations.get(index) else { continue };
                match bitmap.to_mask().map(Shape::Mask) {
                    Some(shape) if shape != before.shape => changes.push(Edit::Replace {
                        index,
                        after: Annotation { shape, pending: false, ..before.clone() },
                        before: before.clone(),
                    }),
                    Some(_) => {}
                    None => removed.push(index),
                }
            }
        } else {
            let shape = Shape::Mask(self.painted.to_mask()?);
            let mut merged = self.merged;
            merged.sort_unstable();
            match merged.split_first() {
                None => changes.push(Edit::Add { index: annotations.len(), annotation: Annotation::manual(self.label, shape) }),
                Some((&first, others)) => {
                    let before = annotations.get(first)?.clone();
                    changes.push(Edit::Replace { index: first, after: Annotation { shape, pending: false, ..before.clone() }, before });
                    removed.extend(others.iter().copied().filter(|&index| index < annotations.len()));
                }
            }
        }
        removed.sort_unstable_by(|a, b| b.cmp(a));
        changes.extend(removed.iter().map(|&index| Edit::Remove { index, annotation: annotations[index].clone() }));
        let edit = match changes.len() {
            0 => return None,
            1 => changes.pop()?,
            _ => Edit::Batch(changes),
        };
        Some(StrokeOutcome { edit, removed })
    }
}

/// The mask of `annotation` if a stroke may change it (not locked or hidden).
fn editable_mask(annotation: &Annotation) -> Option<&crate::mask::Mask> {
    match &annotation.shape {
        Shape::Mask(mask) if !annotation.locked && !annotation.hidden => Some(mask),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: (u32, u32) = (100, 100);

    fn square(left: u32, top: u32, side: u32) -> Bitmap {
        let mut bitmap = Bitmap::default();
        bitmap.paint_square(Point::new(f64::from(left) + f64::from(side) / 2.0, f64::from(top) + f64::from(side) / 2.0), f64::from(side) / 2.0, IMAGE);
        bitmap
    }

    fn mask(label: i64, bitmap: &Bitmap) -> Annotation {
        Annotation::manual(label, Shape::Mask(bitmap.to_mask().expect("pixels")))
    }

    fn mask_of(annotation: &Annotation) -> Bitmap {
        match &annotation.shape {
            Shape::Mask(mask) => mask.to_bitmap(),
            _ => panic!("not a mask"),
        }
    }

    #[test]
    fn brush_merges_into_lowest_same_label_mask() {
        let list = vec![mask(1, &square(10, 10, 10)), mask(2, &square(30, 10, 10)), mask(1, &square(50, 10, 10))];
        let mut stroke = MaskStroke::begin(false, 1, 4.0, IMAGE, Point::new(15.0, 15.0));
        stroke.segment(Point::new(15.0, 15.0), &list);
        stroke.segment(Point::new(55.0, 15.0), &list);
        assert!(stroke.is_merged(0) && stroke.is_merged(2) && !stroke.is_merged(1));
        let outcome = stroke.finish(&list).expect("an edit");
        assert_eq!(outcome.removed, vec![2]);
        let Edit::Batch(changes) = &outcome.edit else { panic!("a batch") };
        let Edit::Replace { index: 0, after, .. } = &changes[0] else { panic!("replaces the lowest") };
        let merged = mask_of(after);
        assert!(merged.get(12, 12) && merged.get(58, 18) && merged.get(35, 15));
        assert!(matches!(changes[1], Edit::Remove { index: 2, .. }));
    }

    #[test]
    fn brush_alone_adds_a_mask() {
        let list = vec![mask(2, &square(10, 10, 10))];
        let mut stroke = MaskStroke::begin(false, 1, 4.0, IMAGE, Point::new(15.0, 15.0));
        stroke.segment(Point::new(15.0, 15.0), &list);
        let outcome = stroke.finish(&list).expect("an edit");
        assert!(matches!(outcome.edit, Edit::Add { index: 1, .. }));
        assert!(outcome.removed.is_empty());
    }

    #[test]
    fn locked_and_hidden_masks_are_untouched() {
        let mut locked = mask(1, &square(10, 10, 10));
        locked.locked = true;
        let mut hidden = mask(1, &square(10, 10, 10));
        hidden.hidden = true;
        let list = vec![locked, hidden];
        for erase in [false, true] {
            let mut stroke = MaskStroke::begin(erase, 1, 30.0, IMAGE, Point::new(15.0, 15.0));
            let change = stroke.segment(Point::new(15.0, 15.0), &list);
            assert!(change.erased.is_empty());
            assert!(!stroke.is_merged(0) && !stroke.is_merged(1));
            let outcome = stroke.finish(&list);
            if erase {
                assert_eq!(outcome, None);
            } else {
                assert!(matches!(outcome.expect("an edit").edit, Edit::Add { index: 2, .. }));
            }
        }
    }

    #[test]
    fn erasing_to_empty_removes_highest_first() {
        let list = vec![mask(1, &square(10, 10, 4)), mask(2, &square(40, 40, 10)), mask(3, &square(12, 12, 4))];
        let mut stroke = MaskStroke::begin(true, 0, 20.0, IMAGE, Point::new(13.0, 13.0));
        let change = stroke.segment(Point::new(13.0, 13.0), &list);
        assert_eq!(change.erased.iter().map(|(index, _)| *index).collect::<Vec<_>>(), vec![0, 2]);
        let outcome = stroke.finish(&list).expect("an edit");
        assert_eq!(outcome.removed, vec![2, 0]);
        let Edit::Batch(changes) = &outcome.edit else { panic!("a batch") };
        assert!(matches!(changes[..], [Edit::Remove { index: 2, .. }, Edit::Remove { index: 0, .. }]));
        assert_eq!(outcome.selection_after(&BTreeSet::from([0, 1, 2])), BTreeSet::from([0]));
    }

    #[test]
    fn partial_erase_replaces_the_mask() {
        let list = vec![mask(1, &square(10, 10, 20))];
        let mut stroke = MaskStroke::begin(true, 0, 4.0, IMAGE, Point::new(12.0, 12.0));
        stroke.segment(Point::new(12.0, 12.0), &list);
        let outcome = stroke.finish(&list).expect("an edit");
        let Edit::Replace { index: 0, after, .. } = &outcome.edit else { panic!("a replace") };
        assert!(!mask_of(after).get(12, 12) && mask_of(after).get(25, 25));
        assert!(outcome.removed.is_empty());
    }

    #[test]
    fn limit_keeps_the_brush_inside_while_on() {
        let mut stroke = MaskStroke::begin(false, 1, 20.0, IMAGE, Point::new(20.0, 20.0));
        stroke.set_limit(square(15, 15, 10));
        stroke.limit(true);
        stroke.segment(Point::new(20.0, 20.0), &[]);
        assert_eq!(stroke.painted().bounds(), Some(PixelRect { left: 15, top: 15, width: 10, height: 10 }));
        stroke.limit(false);
        stroke.segment(Point::new(20.0, 20.0), &[]);
        assert!(stroke.painted().get(12, 20));
    }

    #[test]
    fn fill_region_merges_and_erases() {
        let list = vec![mask(1, &square(10, 10, 10)), mask(1, &square(30, 10, 10))];
        let region = square(15, 10, 20);
        let outcome = MaskStroke::from_region(false, 1, region.clone(), IMAGE, &list).finish(&list).expect("an edit");
        assert_eq!(outcome.removed, vec![1]);
        let outcome = MaskStroke::from_region(true, 1, square(0, 0, 50), IMAGE, &list).finish(&list).expect("an edit");
        assert_eq!(outcome.removed, vec![1, 0]);
    }
}
