//! When the annotation workspace saves an image's annotations: a pure state machine the workspace
//! feeds with edits, timer ticks and save results, and that answers whether to save now.
//!
//! The editor counts its changes in a revision; the policy remembers the revision the server has
//! and the image version the editor is based on. The workspace keeps only the debounce timer,
//! sending a save with the base version it is told, and showing the state.

/// What the save indicator shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveState {
    Saved,
    Unsaved,
    Saving,
    Failed(String),
    /// The image changed elsewhere; saving stops until the user reloads (or a run is merged).
    Conflict(String),
}

/// How a save ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// The server stored it as this version of the image.
    Saved { version: i64 },
    /// The server refused it: the image changed since the base version.
    Stale(String),
    Failed(String),
}

/// What the workspace does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// (Re)start the debounce timer; its firing is reported with [`Autosave::timer_fired`].
    StartTimer,
    /// Save the editor's annotations as they are now, on `base_version`, and report the result
    /// with [`Autosave::save_finished`].
    Save { base_version: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autosave {
    state: SaveState,
    /// The image version the editor's annotations are based on.
    version: i64,
    /// The editor revision the server has.
    saved_revision: u64,
    /// The revision being saved, while a save is in flight.
    in_flight: Option<u64>,
    /// The user left the image: no more timers, only a final save.
    left: bool,
}

impl Autosave {
    /// Annotations just loaded at `version`, with the editor at `revision`.
    pub fn new(version: i64, revision: u64) -> Self {
        Self {
            state: SaveState::Saved,
            version,
            saved_revision: revision,
            in_flight: None,
            left: false,
        }
    }

    pub fn state(&self) -> &SaveState {
        &self.state
    }

    /// Whether the editor at `revision` has changes the server lacks.
    pub fn has_unsaved(&self, revision: u64) -> bool {
        revision != self.saved_revision
    }

    /// The editor changed (edit, undo, redo) and is now at `revision`.
    pub fn edited(&mut self, revision: u64) -> Action {
        if self.left || matches!(self.state, SaveState::Conflict(_)) {
            return Action::Nothing;
        }
        if !self.has_unsaved(revision) {
            if self.state == SaveState::Unsaved {
                // Undone back to what the server has.
                self.state = SaveState::Saved;
            }
            return Action::Nothing;
        }
        if self.state != SaveState::Saving {
            self.state = SaveState::Unsaved;
        }
        Action::StartTimer
    }

    /// The debounce timer fired with the editor at `revision`.
    pub fn timer_fired(&mut self, revision: u64) -> Action {
        if matches!(self.state, SaveState::Saving | SaveState::Conflict(_)) {
            return Action::Nothing;
        }
        if !self.has_unsaved(revision) {
            self.state = SaveState::Saved;
            return Action::Nothing;
        }
        self.start_save(revision)
    }

    /// The user asked to retry a failed save; the editor is at `revision`.
    pub fn retry(&mut self, revision: u64) -> Action {
        self.timer_fired(revision)
    }

    /// The save in flight ended; the editor is now at `revision`.
    pub fn save_finished(&mut self, outcome: SaveOutcome, revision: u64) -> Action {
        let Some(sent) = self.in_flight.take() else {
            return Action::Nothing;
        };
        match outcome {
            SaveOutcome::Saved { version } => {
                self.version = version;
                self.saved_revision = sent;
                self.state = SaveState::Saved;
                if !self.has_unsaved(revision) {
                    Action::Nothing
                } else if self.left {
                    // Changes made before leaving go out at once, on the new version.
                    self.start_save(revision)
                } else {
                    // Edits made while saving are saved next.
                    self.state = SaveState::Unsaved;
                    Action::StartTimer
                }
            }
            SaveOutcome::Stale(message) => {
                self.state = SaveState::Conflict(message);
                Action::Nothing
            }
            SaveOutcome::Failed(message) => {
                self.state = SaveState::Failed(message);
                Action::Nothing
            }
        }
    }

    /// The editor was reset to the server's annotations at `version`; it is now at `revision`.
    pub fn reloaded(&mut self, version: i64, revision: u64) {
        self.version = version;
        self.saved_revision = revision;
        self.state = SaveState::Saved;
    }

    /// A pre-annotation run's results (at image `version`) joined the editor, which went from
    /// revision `before` to `after`. The editor's own unsaved changes are then saved on top of
    /// that version; without any, the editor now matches the server.
    pub fn run_merged(&mut self, version: i64, before: u64, after: u64) -> Action {
        self.version = version;
        if self.state == SaveState::Saved && !self.has_unsaved(before) {
            self.saved_revision = after;
            return Action::Nothing;
        }
        // A save made during the run was refused because of it; the next one is not.
        if matches!(self.state, SaveState::Conflict(_)) {
            self.state = SaveState::Unsaved;
        }
        self.edited(after)
    }

    /// The user left the image (or the page) with the editor at `revision`. A save in flight is
    /// waited for, and anything it misses is sent on the version it returns.
    pub fn left(&mut self, revision: u64) -> Action {
        self.left = true;
        if self.in_flight.is_some()
            || !self.has_unsaved(revision)
            || matches!(self.state, SaveState::Conflict(_))
        {
            return Action::Nothing;
        }
        self.start_save(revision)
    }

    fn start_save(&mut self, revision: u64) -> Action {
        self.state = SaveState::Saving;
        self.in_flight = Some(revision);
        Action::Save {
            base_version: self.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(version: i64) -> SaveOutcome {
        SaveOutcome::Saved { version }
    }

    #[test]
    fn starts_saved() {
        let autosave = Autosave::new(3, 0);
        assert_eq!(autosave.state(), &SaveState::Saved);
        assert!(!autosave.has_unsaved(0));
    }

    #[test]
    fn an_edit_starts_the_timer_and_the_timer_saves() {
        let mut autosave = Autosave::new(3, 0);
        assert_eq!(autosave.edited(1), Action::StartTimer);
        assert_eq!(autosave.state(), &SaveState::Unsaved);
        assert!(autosave.has_unsaved(1));
        assert_eq!(autosave.timer_fired(1), Action::Save { base_version: 3 });
        assert_eq!(autosave.state(), &SaveState::Saving);
        assert_eq!(autosave.save_finished(saved(4), 1), Action::Nothing);
        assert_eq!(autosave.state(), &SaveState::Saved);
        assert!(!autosave.has_unsaved(1));
        // The next save is based on the version the server returned.
        autosave.edited(2);
        assert_eq!(autosave.timer_fired(2), Action::Save { base_version: 4 });
    }

    #[test]
    fn undoing_back_to_the_saved_revision_is_saved() {
        let mut autosave = Autosave::new(3, 5);
        autosave.edited(6);
        assert_eq!(autosave.edited(5), Action::Nothing);
        assert_eq!(autosave.state(), &SaveState::Saved);
        assert_eq!(autosave.timer_fired(5), Action::Nothing);
    }

    #[test]
    fn an_edit_during_a_save_is_saved_after_it() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        autosave.timer_fired(1);
        // Still saving: the timer restarts but its firing does not send a second save.
        assert_eq!(autosave.edited(2), Action::StartTimer);
        assert_eq!(autosave.state(), &SaveState::Saving);
        assert_eq!(autosave.timer_fired(2), Action::Nothing);
        assert_eq!(autosave.save_finished(saved(4), 2), Action::StartTimer);
        assert_eq!(autosave.state(), &SaveState::Unsaved);
        assert!(autosave.has_unsaved(2));
        assert_eq!(autosave.timer_fired(2), Action::Save { base_version: 4 });
    }

    #[test]
    fn a_failed_save_waits_for_a_retry() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        autosave.timer_fired(1);
        assert_eq!(autosave.save_finished(SaveOutcome::Failed("down".into()), 1), Action::Nothing);
        assert_eq!(autosave.state(), &SaveState::Failed("down".into()));
        assert!(autosave.has_unsaved(1));
        assert_eq!(autosave.retry(1), Action::Save { base_version: 3 });
        assert_eq!(autosave.save_finished(saved(4), 1), Action::Nothing);
        assert_eq!(autosave.state(), &SaveState::Saved);
    }

    #[test]
    fn a_conflict_stops_saving_until_reload() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        autosave.timer_fired(1);
        autosave.save_finished(SaveOutcome::Stale("changed".into()), 1);
        assert_eq!(autosave.state(), &SaveState::Conflict("changed".into()));
        assert_eq!(autosave.edited(2), Action::Nothing);
        assert_eq!(autosave.timer_fired(2), Action::Nothing);
        assert_eq!(autosave.retry(2), Action::Nothing);
        assert_eq!(autosave.left(2), Action::Nothing);
        // Reloading resets the editor; its revision becomes the saved one.
        autosave.reloaded(7, 9);
        assert_eq!(autosave.state(), &SaveState::Saved);
        assert!(!autosave.has_unsaved(9));
        autosave.edited(10);
        assert_eq!(autosave.timer_fired(10), Action::Save { base_version: 7 });
    }

    #[test]
    fn a_merged_run_clears_a_conflict_and_saves_on_its_version() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        autosave.timer_fired(1);
        autosave.save_finished(SaveOutcome::Stale("run".into()), 1);
        assert_eq!(autosave.run_merged(5, 1, 2), Action::StartTimer);
        assert_eq!(autosave.state(), &SaveState::Unsaved);
        assert_eq!(autosave.timer_fired(2), Action::Save { base_version: 5 });
    }

    #[test]
    fn a_merged_run_without_own_changes_matches_the_server() {
        let mut autosave = Autosave::new(3, 0);
        assert_eq!(autosave.run_merged(5, 0, 1), Action::Nothing);
        assert_eq!(autosave.state(), &SaveState::Saved);
        assert!(!autosave.has_unsaved(1));
        assert_eq!(autosave.edited(1), Action::Nothing);
        autosave.edited(2);
        assert_eq!(autosave.timer_fired(2), Action::Save { base_version: 5 });
    }

    #[test]
    fn a_merged_run_keeps_own_unsaved_changes_and_saves_them_on_its_version() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        assert_eq!(autosave.run_merged(5, 1, 2), Action::StartTimer);
        assert_eq!(autosave.state(), &SaveState::Unsaved);
        assert_eq!(autosave.timer_fired(2), Action::Save { base_version: 5 });
    }

    #[test]
    fn leaving_with_unsaved_changes_saves_at_once() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        assert_eq!(autosave.left(1), Action::Save { base_version: 3 });
        assert_eq!(autosave.save_finished(saved(4), 1), Action::Nothing);
        assert_eq!(autosave.state(), &SaveState::Saved);
    }

    #[test]
    fn leaving_when_saved_does_nothing() {
        let mut autosave = Autosave::new(3, 0);
        assert_eq!(autosave.left(0), Action::Nothing);
    }

    #[test]
    fn leaving_during_a_save_waits_and_sends_the_rest_on_the_new_version() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        autosave.timer_fired(1);
        autosave.edited(2);
        // No second save on the stale version 3.
        assert_eq!(autosave.left(2), Action::Nothing);
        assert_eq!(autosave.save_finished(saved(4), 2), Action::Save { base_version: 4 });
        assert_eq!(autosave.save_finished(saved(5), 2), Action::Nothing);
        assert_eq!(autosave.state(), &SaveState::Saved);
    }

    #[test]
    fn leaving_during_a_save_that_covers_everything_sends_nothing_more() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        autosave.timer_fired(1);
        assert_eq!(autosave.left(1), Action::Nothing);
        assert_eq!(autosave.save_finished(saved(4), 1), Action::Nothing);
    }

    #[test]
    fn leaving_during_a_save_that_fails_sends_nothing_more() {
        let mut autosave = Autosave::new(3, 0);
        autosave.edited(1);
        autosave.timer_fired(1);
        autosave.left(2);
        assert_eq!(autosave.save_finished(SaveOutcome::Stale("x".into()), 2), Action::Nothing);
        assert_eq!(autosave.save_finished(saved(9), 2), Action::Nothing);
    }

    #[test]
    fn no_timer_after_leaving() {
        let mut autosave = Autosave::new(3, 0);
        autosave.left(0);
        assert_eq!(autosave.edited(1), Action::Nothing);
    }

    #[test]
    fn a_result_without_a_save_in_flight_is_ignored() {
        let mut autosave = Autosave::new(3, 0);
        assert_eq!(autosave.save_finished(saved(9), 0), Action::Nothing);
        assert_eq!(autosave, Autosave::new(3, 0));
    }
}
