//! Stack-allocated move list for zero-allocation movegen

use fusion_core::Move;

/// Maximum moves per piece (theoretical max is ~80, use 256 for safety)
pub const MAX_MOVES: usize = 256;

/// Fixed-capacity move list - no heap allocation
#[derive(Clone)]
pub struct MoveList {
    moves: [Move; MAX_MOVES],
    len: usize,
}

impl MoveList {
    /// Create empty move list
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            moves: [Move::ZERO; MAX_MOVES],
            len: 0,
        }
    }

    /// Push a move (panics in debug if overflow)
    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        debug_assert!(self.len < MAX_MOVES, "MoveList overflow");
        self.moves[self.len] = m;
        self.len += 1;
    }

    /// Current length
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clear the list
    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Get slice of valid moves
    #[inline(always)]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }

    /// Iterate over moves
    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = &Move> {
        self.moves[..self.len].iter()
    }

    /// Convert to Vec (for compatibility)
    pub fn to_vec(&self) -> Vec<Move> {
        self.as_slice().to_vec()
    }
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> IntoIterator for &'a MoveList {
    type Item = &'a Move;
    type IntoIter = std::slice::Iter<'a, Move>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl IntoIterator for MoveList {
    type Item = Move;
    type IntoIter = MoveListIntoIter;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        MoveListIntoIter { list: self, pos: 0 }
    }
}

pub struct MoveListIntoIter {
    list: MoveList,
    pos: usize,
}

impl Iterator for MoveListIntoIter {
    type Item = Move;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.list.len {
            let m = self.list.moves[self.pos];
            self.pos += 1;
            Some(m)
        } else {
            None
        }
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.list.len - self.pos;
        (remaining, Some(remaining))
    }
}

impl std::ops::Index<usize> for MoveList {
    type Output = Move;

    #[inline(always)]
    fn index(&self, index: usize) -> &Self::Output {
        debug_assert!(index < self.len, "MoveList index out of bounds");
        &self.moves[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::{Piece, Rotation};

    #[test]
    fn test_empty_list() {
        let list = MoveList::new();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_push_and_iterate() {
        let mut list = MoveList::new();
        let m1 = Move::new(Piece::T, Rotation::North, 3, 0);
        let m2 = Move::new(Piece::T, Rotation::East, 4, 0);
        list.push(m1);
        list.push(m2);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0], m1);
        assert_eq!(list[1], m2);
    }

    #[test]
    fn test_clear() {
        let mut list = MoveList::new();
        list.push(Move::new(Piece::I, Rotation::North, 0, 0));
        list.clear();
        assert!(list.is_empty());
    }

    #[test]
    fn test_to_vec() {
        let mut list = MoveList::new();
        list.push(Move::new(Piece::O, Rotation::North, 4, 0));
        let vec = list.to_vec();
        assert_eq!(vec.len(), 1);
    }
}
