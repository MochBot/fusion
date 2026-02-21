use crate::board::{Board, BOARD_HEIGHT};
use crate::header::COL_NB;

const ZOBRIST_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
pub(crate) const DEFAULT_TT_SIZE: usize = 65_536;

#[derive(Clone)]
pub(crate) struct ZobristKeys {
    keys: [[u64; BOARD_HEIGHT]; COL_NB],
}

impl ZobristKeys {
    pub(crate) fn new() -> Self {
        let mut rng = SplitMix64::new(ZOBRIST_SEED);
        let mut keys = [[0u64; BOARD_HEIGHT]; COL_NB];

        for row in keys.iter_mut().take(COL_NB) {
            for key in row.iter_mut().take(BOARD_HEIGHT) {
                *key = rng.next_u64();
            }
        }

        Self { keys }
    }

    pub(crate) fn hash_board(&self, board: &Board) -> u64 {
        let mut hash = 0u64;

        for y in 0..BOARD_HEIGHT {
            let row = board.rows[y];
            for x in 0..COL_NB {
                if row & (1u16 << x) != 0 {
                    hash ^= self.keys[x][y];
                }
            }
        }

        hash
    }
}

impl Default for ZobristKeys {
    fn default() -> Self {
        Self::new()
    }
}

/// Cached static ZobristKeys — initialized once, reused across all search calls.
/// Avoids re-computing 400 random keys per find_best_move invocation.
pub(crate) fn get_zobrist_keys() -> &'static ZobristKeys {
    use std::sync::OnceLock;
    static KEYS: OnceLock<ZobristKeys> = OnceLock::new();
    KEYS.get_or_init(ZobristKeys::new)
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub(crate) struct TTEntry {
    pub(crate) hash: u64,
    pub(crate) depth: u8,
    pub(crate) score: f32,
}

pub(crate) struct TranspositionTable {
    entries: Box<[TTEntry]>,
}

impl TranspositionTable {
    pub(crate) fn new(size: usize) -> Self {
        let size = size.max(1);
        Self {
            entries: vec![TTEntry::default(); size].into_boxed_slice(),
        }
    }

    #[inline]
    fn index(&self, hash: u64) -> usize {
        (hash as usize) % self.entries.len()
    }

    pub(crate) fn probe(&self, hash: u64, depth: u8) -> Option<f32> {
        let entry = self.entries[self.index(hash)];
        if entry.hash == hash && entry.depth >= depth {
            Some(entry.score)
        } else {
            None
        }
    }

    pub(crate) fn store(&mut self, hash: u64, depth: u8, score: f32) {
        let entry = &mut self.entries[self.index(hash)];
        if depth >= entry.depth {
            *entry = TTEntry { hash, depth, score };
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.fill(TTEntry::default());
    }
}

impl Default for TranspositionTable {
    fn default() -> Self {
        Self::new(DEFAULT_TT_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_with_cells(cells: &[(usize, usize)]) -> Board {
        let mut board = Board::new();
        for &(x, y) in cells {
            board.rows[y] |= 1u16 << x;
            board.cols[x] |= 1u64 << y;
        }
        board
    }

    #[test]
    fn tt_entry_layout_is_16_bytes() {
        assert_eq!(std::mem::size_of::<TTEntry>(), 16);
    }

    #[test]
    fn hash_is_consistent_for_same_board() {
        let keys_a = ZobristKeys::new();
        let keys_b = ZobristKeys::new();
        let board = board_with_cells(&[(0, 0), (4, 5), (9, 12), (2, 39)]);

        assert_eq!(keys_a.hash_board(&board), keys_b.hash_board(&board));
    }

    #[test]
    fn hash_changes_when_board_changes() {
        let keys = ZobristKeys::new();
        let board_a = board_with_cells(&[(1, 1), (3, 3), (5, 5)]);
        let board_b = board_with_cells(&[(1, 1), (3, 3), (5, 5), (7, 7)]);

        assert_ne!(keys.hash_board(&board_a), keys.hash_board(&board_b));
    }

    #[test]
    fn tt_store_probe_round_trip() {
        let mut tt = TranspositionTable::new(1024);
        let hash = 0x1234_5678_9ABC_DEF0;
        let score = 12.5;

        tt.store(hash, 6, score);

        assert_eq!(tt.probe(hash, 6), Some(score));
        assert_eq!(tt.probe(hash, 4), Some(score));
        assert_eq!(tt.probe(hash, 7), None);
    }

    #[test]
    fn tt_uses_depth_preferred_replacement() {
        let mut tt = TranspositionTable::new(1);
        let hash_a = 0xAAAA_AAAA_AAAA_AAAA;
        let hash_b = 0xBBBB_BBBB_BBBB_BBBB;

        tt.store(hash_a, 6, 1.0);
        tt.store(hash_b, 5, 2.0);

        assert_eq!(tt.probe(hash_a, 6), Some(1.0));
        assert_eq!(tt.probe(hash_b, 5), None);

        tt.store(hash_b, 6, 3.0);

        assert_eq!(tt.probe(hash_b, 6), Some(3.0));
        assert_eq!(tt.probe(hash_a, 6), None);
    }

    #[test]
    fn tt_miss_on_different_hash() {
        let mut tt = TranspositionTable::new(1024);
        tt.store(0xDEAD_BEEF_DEAD_BEEF, 4, 7.25);

        assert_eq!(tt.probe(0x1234_5678_1234_5678, 4), None);
    }

    #[test]
    fn tt_clear_resets_entries() {
        let mut tt = TranspositionTable::new(1024);
        let hash = 0x0F0F_F0F0_0F0F_F0F0;
        tt.store(hash, 5, 9.0);

        assert_eq!(tt.probe(hash, 5), Some(9.0));
        tt.clear();
        assert_eq!(tt.probe(hash, 5), None);
    }
}
