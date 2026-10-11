//! Undo and redo for the annotations of one image. Every change goes through `History::apply`,
//! which records how to reverse it.

use crate::annotation::Annotation;

/// One reversible change to an image's annotation list.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Add { index: usize, annotation: Annotation },
    Remove { index: usize, annotation: Annotation },
    Replace { index: usize, before: Annotation, after: Annotation },
}

impl Edit {
    fn inverse(&self) -> Edit {
        match self.clone() {
            Edit::Add { index, annotation } => Edit::Remove { index, annotation },
            Edit::Remove { index, annotation } => Edit::Add { index, annotation },
            Edit::Replace { index, before, after } => Edit::Replace { index, before: after, after: before },
        }
    }

    fn perform(&self, annotations: &mut Vec<Annotation>) {
        match self {
            Edit::Add { index, annotation } => annotations.insert(*index, annotation.clone()),
            Edit::Remove { index, .. } => {
                annotations.remove(*index);
            }
            Edit::Replace { index, after, .. } => annotations[*index] = after.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct History {
    done: Vec<Edit>,
    undone: Vec<Edit>,
}

impl History {
    /// Performs `edit` and records it. A new edit discards anything that could be redone.
    pub fn apply(&mut self, annotations: &mut Vec<Annotation>, edit: Edit) {
        edit.perform(annotations);
        self.done.push(edit);
        self.undone.clear();
    }

    /// Reverses the last edit. Returns false if there is nothing to undo.
    pub fn undo(&mut self, annotations: &mut Vec<Annotation>) -> bool {
        let Some(edit) = self.done.pop() else { return false };
        edit.inverse().perform(annotations);
        self.undone.push(edit);
        true
    }

    /// Performs the last undone edit again. Returns false if there is nothing to redo.
    pub fn redo(&mut self, annotations: &mut Vec<Annotation>) -> bool {
        let Some(edit) = self.undone.pop() else { return false };
        edit.perform(annotations);
        self.done.push(edit);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        annotation::Shape,
        geometry::Rect,
    };

    fn boxed(label: i64, x: f64) -> Annotation {
        Annotation::manual(label, Shape::Rectangle(Rect::new(x, 0.0, x + 10.0, 10.0)))
    }

    #[test]
    fn edits_can_be_undone_and_redone_in_order() {
        let mut annotations = vec![boxed(1, 0.0)];
        let mut history = History::default();
        history.apply(&mut annotations, Edit::Add { index: 1, annotation: boxed(2, 20.0) });
        history.apply(&mut annotations, Edit::Replace { index: 0, before: boxed(1, 0.0), after: boxed(1, 5.0) });
        history.apply(&mut annotations, Edit::Remove { index: 1, annotation: boxed(2, 20.0) });
        assert_eq!(annotations, vec![boxed(1, 5.0)]);

        assert!(history.undo(&mut annotations));
        assert_eq!(annotations, vec![boxed(1, 5.0), boxed(2, 20.0)]);
        assert!(history.undo(&mut annotations));
        assert_eq!(annotations, vec![boxed(1, 0.0), boxed(2, 20.0)]);
        assert!(history.undo(&mut annotations));
        assert_eq!(annotations, vec![boxed(1, 0.0)]);
        assert!(!history.undo(&mut annotations));

        assert!(history.redo(&mut annotations));
        assert!(history.redo(&mut annotations));
        assert!(history.redo(&mut annotations));
        assert_eq!(annotations, vec![boxed(1, 5.0)]);
        assert!(!history.redo(&mut annotations));
    }

    #[test]
    fn a_new_edit_after_undo_discards_the_redo_branch() {
        let mut annotations = Vec::new();
        let mut history = History::default();
        history.apply(&mut annotations, Edit::Add { index: 0, annotation: boxed(1, 0.0) });
        history.undo(&mut annotations);
        assert!(history.can_redo());
        history.apply(&mut annotations, Edit::Add { index: 0, annotation: boxed(2, 0.0) });
        assert!(!history.can_redo());
        assert_eq!(annotations, vec![boxed(2, 0.0)]);
    }

    #[test]
    fn removing_from_the_middle_restores_the_same_position() {
        let mut annotations = vec![boxed(1, 0.0), boxed(2, 20.0), boxed(3, 40.0)];
        let mut history = History::default();
        history.apply(&mut annotations, Edit::Remove { index: 1, annotation: boxed(2, 20.0) });
        history.undo(&mut annotations);
        assert_eq!(annotations, vec![boxed(1, 0.0), boxed(2, 20.0), boxed(3, 40.0)]);
    }
}
