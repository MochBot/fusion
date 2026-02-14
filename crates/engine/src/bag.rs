use fusion_core::Piece;

pub struct SevenBag {
    pieces: Vec<Piece>,
    index: usize,
}

impl SevenBag {
    pub fn new(pieces: &[Piece]) -> Self {
        Self {
            pieces: pieces.to_vec(),
            index: 0,
        }
    }

    pub fn next_piece(&mut self) -> Option<Piece> {
        if self.index < self.pieces.len() {
            let piece = self.pieces[self.index];
            self.index += 1;
            Some(piece)
        } else {
            None
        }
    }

    pub fn peek(&self) -> Option<Piece> {
        if self.index < self.pieces.len() {
            Some(self.pieces[self.index])
        } else {
            None
        }
    }

    pub fn remaining(&self) -> &[Piece] {
        &self.pieces[self.index..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bag_creation() {
        let bag = SevenBag::new(&Piece::ALL);
        assert_eq!(bag.remaining().len(), 7);
    }

    #[test]
    fn test_bag_next() {
        let mut bag = SevenBag::new(&Piece::ALL);
        for i in 0..7 {
            let expected = Piece::ALL[i];
            assert_eq!(bag.next_piece(), Some(expected));
        }
        assert_eq!(bag.next_piece(), None);
    }

    #[test]
    fn test_bag_peek() {
        let bag = SevenBag::new(&Piece::ALL);
        assert_eq!(bag.peek(), Some(Piece::ALL[0]));
        // ensure peek doesn't consume
        assert_eq!(bag.peek(), Some(Piece::ALL[0]));
    }

    #[test]
    fn test_bag_remaining() {
        let mut bag = SevenBag::new(&Piece::ALL);
        assert_eq!(bag.remaining().len(), 7);
        let _ = bag.next_piece();
        assert_eq!(bag.remaining().len(), 6);
        assert_eq!(bag.remaining()[0], Piece::ALL[1]);
    }
}
