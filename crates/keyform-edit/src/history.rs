//! Linear undo/redo history of document states.
//!
//! spec: 50-ui/undo.md. Each step stores the document state *after* it
//! (model snapshot, selection, edit level, …). Model objects are shared via
//! `Arc`, so a snapshot costs a few pointer copies.

/// Default capacity in steps (undo.md §1).
pub const CAPACITY: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// Changes document data: counts as a modification.
    Real,
    /// Only the selection changed (undo.md §3).
    SelectionOnly,
}

struct Step<S> {
    name: String,
    kind: StepKind,
    state: S,
    /// State undo returns to, when it isn't the previous step's: set when
    /// selection-only steps were folded into this one, so undo still
    /// restores the selection from before the step (undo.md §2).
    before: Option<S>,
}

/// History of states `S` with a cursor. Index 0 is the state the history
/// started from (or the oldest kept state after trimming).
pub struct History<S> {
    steps: Vec<Step<S>>,
    cursor: usize,
    capacity: usize,
    /// Incremented by every real step, undo and redo of a real step: lets the
    /// owner compare against the value at the last save.
    revision: u64,
    /// Revision number of each step's state.
    revisions: Vec<u64>,
    next_revision: u64,
}

impl<S: Clone> History<S> {
    pub fn new(initial: S) -> Self {
        Self::with_capacity(initial, CAPACITY)
    }

    pub fn with_capacity(initial: S, capacity: usize) -> Self {
        Self {
            steps: vec![Step {
                name: String::new(),
                kind: StepKind::Real,
                state: initial,
                before: None,
            }],
            cursor: 0,
            capacity: capacity.max(1),
            revision: 0,
            revisions: vec![0],
            next_revision: 1,
        }
    }

    /// The current state.
    pub fn current(&self) -> &S {
        &self.steps[self.cursor].state
    }

    /// Revision of the current state; equal revisions mean equal document data.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Records a step ending in `state`. `unchanged` tells whether the step
    /// recorded no change; such steps are discarded (undo.md §2). Returns
    /// whether the step was kept.
    pub fn commit(&mut self, name: &str, kind: StepKind, state: S, unchanged: bool) -> bool {
        if unchanged {
            return false;
        }
        // Adding a step removes the redo tail.
        self.steps.truncate(self.cursor + 1);
        self.revisions.truncate(self.cursor + 1);
        // A real step first removes the selection-only steps directly
        // before the cursor (undo.md §3); the first state always stays.
        let mut before = None;
        if kind == StepKind::Real {
            while self.steps.len() > 1
                && self
                    .steps
                    .last()
                    .is_some_and(|s| s.kind == StepKind::SelectionOnly)
            {
                let removed = self.steps.pop();
                self.revisions.pop();
                // The newest removed state is what this step started from.
                if before.is_none() {
                    before = removed.map(|s| s.state);
                }
            }
        }
        let rev = if kind == StepKind::Real {
            let r = self.next_revision;
            self.next_revision += 1;
            r
        } else {
            *self.revisions.last().unwrap_or(&0)
        };
        self.steps.push(Step {
            name: name.to_owned(),
            kind,
            state,
            before,
        });
        self.revisions.push(rev);
        // Capacity: drop the oldest steps.
        while self.steps.len() > self.capacity + 1 {
            self.steps.remove(0);
            self.revisions.remove(0);
        }
        self.cursor = self.steps.len() - 1;
        self.revision = rev;
        true
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.steps.len()
    }

    /// Name of the step undo would revert.
    pub fn undo_name(&self) -> Option<&str> {
        self.can_undo()
            .then(|| self.steps[self.cursor].name.as_str())
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.steps.get(self.cursor + 1).map(|s| s.name.as_str())
    }

    /// Moves back one step and returns the state to restore.
    pub fn undo(&mut self) -> Option<&S> {
        if !self.can_undo() {
            return None;
        }
        self.cursor -= 1;
        self.revision = self.revisions[self.cursor];
        let undone = &self.steps[self.cursor + 1];
        Some(
            undone
                .before
                .as_ref()
                .unwrap_or(&self.steps[self.cursor].state),
        )
    }

    pub fn redo(&mut self) -> Option<&S> {
        if !self.can_redo() {
            return None;
        }
        self.cursor += 1;
        self.revision = self.revisions[self.cursor];
        Some(&self.steps[self.cursor].state)
    }

    pub fn len(&self) -> usize {
        self.steps.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_history_with_redo_tail() {
        let mut h = History::new(0);
        assert!(h.commit("a", StepKind::Real, 1, false));
        assert!(h.commit("b", StepKind::Real, 2, false));
        assert_eq!(h.undo_name(), Some("b"));
        assert_eq!(h.undo(), Some(&1));
        assert_eq!(h.redo_name(), Some("b"));
        // A new step removes the redo tail.
        h.commit("c", StepKind::Real, 3, false);
        assert!(!h.can_redo());
        assert_eq!(h.undo(), Some(&1));
        assert_eq!(h.undo(), Some(&0));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some(&1));
    }

    #[test]
    fn unchanged_steps_are_discarded() {
        let mut h = History::new(0);
        assert!(!h.commit("noop", StepKind::Real, 0, true));
        assert!(h.is_empty());
    }

    #[test]
    fn selection_steps_collapse_before_real_work() {
        let mut h = History::new("s0".to_owned());
        h.commit("select", StepKind::SelectionOnly, "s1".into(), false);
        h.commit("select", StepKind::SelectionOnly, "s2".into(), false);
        // Selection steps can be undone…
        assert_eq!(h.undo().cloned(), Some("s1".into()));
        h.redo();
        // …but disappear once a real step follows.
        h.commit("edit", StepKind::Real, "e".into(), false);
        assert_eq!(h.len(), 1);
        // Undo restores the selection the edit started from.
        assert_eq!(h.undo().cloned(), Some("s2".into()));
        assert_eq!(h.redo().cloned(), Some("e".into()));
    }

    #[test]
    fn revisions_track_modification() {
        let mut h = History::new(0);
        let saved = h.revision();
        h.commit("a", StepKind::Real, 1, false);
        assert_ne!(h.revision(), saved);
        h.undo();
        assert_eq!(h.revision(), saved);
        h.commit("sel", StepKind::SelectionOnly, 0, false);
        assert_eq!(h.revision(), saved, "selection-only steps don't modify");
        h.redo();
    }

    #[test]
    fn capacity_drops_oldest() {
        let mut h = History::with_capacity(0, 3);
        for i in 1..=5 {
            h.commit("s", StepKind::Real, i, false);
        }
        assert_eq!(h.len(), 3);
        let mut last = None;
        while let Some(s) = h.undo() {
            last = Some(*s);
        }
        assert_eq!(last, Some(2));
    }
}
