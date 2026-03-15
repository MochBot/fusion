use crate::header::{Piece, ALL_PIECES, PIECE_NB};

#[derive(Debug, Clone)]
pub(crate) struct BagTracker {
    seen: [bool; PIECE_NB],
    count: u8,
}

impl BagTracker {
    pub(crate) fn new() -> Self {
        Self {
            seen: [false; PIECE_NB],
            count: 0,
        }
    }

    pub(crate) fn consume(&mut self, piece: Piece) {
        if self.count >= 7 {
            self.reset();
        }
        let idx = piece as usize;
        // If already seen, we've crossed a bag boundary — reset first.
        if self.seen[idx] {
            self.reset();
        }
        self.seen[idx] = true;
        self.count += 1;
    }

    pub(crate) fn remaining(&self) -> Vec<Piece> {
        ALL_PIECES
            .iter()
            .copied()
            .filter(|&p| !self.seen[p as usize])
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn predict_next(&mut self, queue: &[Piece]) -> Vec<Piece> {
        for &piece in queue {
            self.consume(piece);
        }
        self.remaining()
    }

    #[allow(dead_code)]
    pub(crate) fn count(&self) -> u8 {
        self.count
    }

    fn reset(&mut self) {
        self.seen = [false; PIECE_NB];
        self.count = 0;
    }
}

impl Default for BagTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn extend_queue(queue: &[Piece], current: Piece, hold: Option<Piece>) -> Vec<Piece> {
    let mut tracker = BagTracker::new();

    if let Some(h) = hold {
        tracker.consume(h);
    }

    tracker.consume(current);

    for &piece in queue {
        tracker.consume(piece);
    }

    let remaining = tracker.remaining();
    let mut extended = queue.to_vec();

    // Only predict when ≤2 pieces remain — those are guaranteed to appear
    // before the next bag, though their order is unknown.
    if remaining.len() <= 2 && !remaining.is_empty() {
        extended.extend_from_slice(&remaining);
    }

    extended
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::Piece::*;

    #[test]
    fn test_new_tracker_has_all_remaining() {
        let tracker = BagTracker::new();
        let remaining = tracker.remaining();
        assert_eq!(remaining.len(), 7);
        assert_eq!(remaining, vec![I, O, T, L, J, S, Z]);
    }

    #[test]
    fn test_consume_reduces_remaining() {
        let mut tracker = BagTracker::new();
        tracker.consume(I);
        tracker.consume(T);
        tracker.consume(S);

        let remaining = tracker.remaining();
        assert_eq!(remaining.len(), 4);
        assert!(!remaining.contains(&I));
        assert!(!remaining.contains(&T));
        assert!(!remaining.contains(&S));
        assert!(remaining.contains(&O));
        assert!(remaining.contains(&L));
        assert!(remaining.contains(&J));
        assert!(remaining.contains(&Z));
    }

    #[test]
    fn test_full_bag_cycle_resets() {
        let mut tracker = BagTracker::new();
        // Consume all 7
        for &p in &ALL_PIECES {
            tracker.consume(p);
        }
        assert_eq!(tracker.count(), 7);

        // Next consume should trigger reset
        tracker.consume(T);
        assert_eq!(tracker.count(), 1);
        let remaining = tracker.remaining();
        assert_eq!(remaining.len(), 6);
        assert!(!remaining.contains(&T));
    }

    #[test]
    fn test_remaining_after_consuming_5_pieces() {
        let mut tracker = BagTracker::new();
        tracker.consume(I);
        tracker.consume(O);
        tracker.consume(T);
        tracker.consume(L);
        tracker.consume(J);

        let remaining = tracker.remaining();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining, vec![S, Z]);
    }

    #[test]
    fn test_predict_next_consumes_and_returns_remaining() {
        let mut tracker = BagTracker::new();
        tracker.consume(I);
        tracker.consume(O);

        let queue = vec![T, L, J];
        let remaining = tracker.predict_next(&queue);
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining, vec![S, Z]);
    }

    #[test]
    fn test_duplicate_piece_triggers_bag_reset() {
        let mut tracker = BagTracker::new();
        tracker.consume(I);
        tracker.consume(O);
        tracker.consume(T);

        // Duplicate I means new bag
        tracker.consume(I);
        assert_eq!(tracker.count(), 1);
        let remaining = tracker.remaining();
        assert_eq!(remaining.len(), 6);
        assert!(!remaining.contains(&I));
    }

    #[test]
    fn test_extend_queue_adds_predicted_pieces() {
        // Setup: hold=I, current=O, queue=[T, L, J]
        // That's 5 pieces consumed from the bag, 2 remaining (S, Z)
        let queue = vec![T, L, J];
        let extended = extend_queue(&queue, O, Some(I));

        // Original queue + predicted S, Z
        assert_eq!(extended.len(), 5);
        assert_eq!(&extended[..3], &[T, L, J]);
        assert!(extended[3..].contains(&S));
        assert!(extended[3..].contains(&Z));
    }

    #[test]
    fn test_extend_queue_no_prediction_when_too_many_remain() {
        // hold=I, current=O, queue=[T]
        // That's 3 pieces consumed, 4 remaining — too uncertain
        let queue = vec![T];
        let extended = extend_queue(&queue, O, Some(I));

        // No prediction appended
        assert_eq!(extended, vec![T]);
    }

    #[test]
    fn test_extend_queue_single_remaining() {
        // hold=I, current=O, queue=[T, L, J, S]
        // That's 6 pieces consumed, 1 remaining (Z) — guaranteed
        let queue = vec![T, L, J, S];
        let extended = extend_queue(&queue, O, Some(I));

        assert_eq!(extended.len(), 5);
        assert_eq!(&extended[..4], &[T, L, J, S]);
        assert_eq!(extended[4], Z);
    }

    #[test]
    fn test_extend_queue_no_hold() {
        // No hold, current=I, queue=[O, T, L, J]
        // That's 5 pieces consumed, 2 remaining (S, Z)
        let queue = vec![O, T, L, J];
        let extended = extend_queue(&queue, I, None);

        assert_eq!(extended.len(), 6);
        assert_eq!(&extended[..4], &[O, T, L, J]);
        assert!(extended[4..].contains(&S));
        assert!(extended[4..].contains(&Z));
    }

    #[test]
    fn test_extend_queue_full_bag_no_prediction() {
        // hold=I, current=O, queue=[T, L, J, S, Z]
        // That's all 7 consumed — bag complete, nothing remaining
        let queue = vec![T, L, J, S, Z];
        let extended = extend_queue(&queue, O, Some(I));

        // No prediction (0 remaining)
        assert_eq!(extended, vec![T, L, J, S, Z]);
    }

    #[test]
    fn test_extend_queue_cross_bag_boundary() {
        // hold=I, current=O, queue=[T, L, J, S, Z, I]
        // First 7 fill bag 1, then I starts bag 2 → 6 remaining in new bag
        // Too many to predict
        let queue = vec![T, L, J, S, Z, I];
        let extended = extend_queue(&queue, O, Some(I));

        // No prediction appended (6 remaining in new bag)
        assert_eq!(extended, vec![T, L, J, S, Z, I]);
    }

    #[test]
    fn test_bag_tracker_default() {
        let tracker: BagTracker = Default::default();
        assert_eq!(tracker.count(), 0);
        assert_eq!(tracker.remaining().len(), 7);
    }
}
