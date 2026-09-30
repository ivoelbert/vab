//! Undo and redo by snapshot: what the editor edits is small, so each step keeps a whole copy.

/// Steps kept for undo.
const LIMIT: usize = 100;

pub struct History<T> {
    /// Snapshots from before each edit, newest last.
    undo: Vec<T>,
    /// Snapshots from before each undo, newest last; cleared by a new edit.
    redo: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl<T: PartialEq> History<T> {
    /// Keeps `before` as an undo step if `current` differs from it; returns whether it did.
    pub fn record(&mut self, before: T, current: &T) -> bool {
        if before == *current {
            return false;
        }
        self.undo.push(before);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    /// Puts back the snapshot from before the last edit; returns whether there was one.
    pub fn undo(&mut self, current: &mut T) -> bool {
        let Some(before) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(current, before));
        true
    }

    /// Reapplies the last undone edit; returns whether there was one.
    pub fn redo(&mut self, current: &mut T) -> bool {
        let Some(after) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(current, after));
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
