use std::collections::VecDeque;

use crate::steps::transaction::Transaction;

pub struct History {
    done: VecDeque<Transaction>,
    redo: Vec<Transaction>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            done: VecDeque::new(),
            redo: Vec::new(),
            capacity,
        }
    }

    pub fn push(&mut self, txn: Transaction) {
        if self.done.len() >= self.capacity {
            self.done.pop_front();
        }
        self.done.push_back(txn);
        self.redo.clear();
    }

    /// Move the most-recent transaction from `done` to `redo`. Returns a reference to it (now in redo).
    pub fn undo(&mut self) -> Option<&Transaction> {
        if let Some(txn) = self.done.pop_back() {
            self.redo.push(txn);
            self.redo.last()
        } else {
            None
        }
    }

    /// Move the last-undone transaction from `redo` back to `done`. Returns a reference to it (now in done).
    pub fn redo(&mut self) -> Option<&Transaction> {
        if let Some(txn) = self.redo.pop() {
            self.done.push_back(txn);
            self.done.back()
        } else {
            None
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
