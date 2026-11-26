//! [`DetourQueue`]: items a detour hands to a later serial point.

use parking_lot::Mutex;

/// Items a detour pushes from whatever thread the engine calls it on, for a later serial point
/// (an input tick's game stage, a post-simulation update) to take and act on where acting is
/// safe. The queue's lock is held only to push or to swap the items out: [`DetourQueue::take`]
/// hands the items over unlocked, so acting on them may reach a detour that pushes again, and that
/// item waits for the next take.
pub struct DetourQueue<T> {
    items: Mutex<Vec<T>>,
}

impl<T> DetourQueue<T> {
    pub const fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
        }
    }

    /// Queue `item` behind those already waiting.
    pub fn push(&self, item: T) {
        self.items.lock().push(item);
    }

    /// Take every waiting item, oldest first, leaving the queue empty.
    pub fn take(&self) -> Vec<T> {
        std::mem::take(&mut *self.items.lock())
    }

    /// Put `items` back ahead of any pushed since they were taken: for items the taker could not
    /// act on yet and tries again at the next take.
    pub fn requeue(&self, mut items: Vec<T>) {
        if items.is_empty() {
            return;
        }
        let mut queued = self.items.lock();
        items.append(&mut queued);
        *queued = items;
    }

    /// Keep at most the `limit` oldest items, dropping the rest.
    pub fn truncate(&self, limit: usize) {
        self.items.lock().truncate(limit);
    }

    /// Drop every waiting item: for a stand-down.
    pub fn clear(&self) {
        self.items.lock().clear();
    }

    pub fn len(&self) -> usize {
        self.items.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.lock().is_empty()
    }
}

impl<T> Default for DetourQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_hands_over_in_order_and_empties_the_queue() {
        let queue = DetourQueue::new();
        queue.push(1);
        queue.push(2);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.take(), [1, 2]);
        assert!(queue.is_empty());
    }

    #[test]
    fn requeued_items_go_ahead_of_newer_ones() {
        let queue = DetourQueue::new();
        queue.push(1);
        queue.push(2);
        let taken = queue.take();
        // An item pushed while the taken ones were acted on.
        queue.push(3);
        queue.requeue(taken);
        queue.truncate(2);
        assert_eq!(queue.take(), [1, 2]);
    }

    #[test]
    fn acting_on_taken_items_may_push_again() {
        let queue = DetourQueue::new();
        queue.push(1);
        for item in queue.take() {
            queue.push(item + 1);
        }
        assert_eq!(queue.take(), [2]);
    }
}
