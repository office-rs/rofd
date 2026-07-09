// TEMPORARY stub - Task 4 replaces this with the real History (transactions, undo/redo stacks).
// Kept minimal so the Editor can construct in Task 2.
pub struct History;
impl History {
    pub fn new(_capacity: usize) -> Self { Self }
    pub fn can_undo(&self) -> bool { false }
    pub fn can_redo(&self) -> bool { false }
}
