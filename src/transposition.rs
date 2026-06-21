use crate::board::{Board, BOARD_HEIGHT};
use crate::header::COL_NB;

const ZOBRIST_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
pub(crate) const DEFAULT_TT_SIZE: usize = 65_536;

const ROW_KEY_COUNT: usize = 1 << COL_NB;
const ROW_MASK: usize = ROW_KEY_COUNT - 1;

#[derive(Clone)]
pub(crate) struct ZobristKeys {
    keys: Box<[[u64; ROW_KEY_COUNT]]>,
}

impl ZobristKeys {
    pub(crate) fn new() -> Self {
        let mut rng = SplitMix64::new(ZOBRIST_SEED);
        let mut per_bit_keys = [[0u64; BOARD_HEIGHT]; COL_NB];

        for row in per_bit_keys.iter_mut().take(COL_NB) {
            for key in row.iter_mut().take(BOARD_HEIGHT) {
                *key = rng.next_u64();
            }
        }

        let mut keys = vec![[0u64; ROW_KEY_COUNT]; BOARD_HEIGHT].into_boxed_slice();
        for y in 0..BOARD_HEIGHT {
            for row_value in 1..ROW_KEY_COUNT {
                let mut hash = 0u64;
                for (x, bit_keys) in per_bit_keys.iter().enumerate().take(COL_NB) {
                    if row_value & (1usize << x) != 0 {
                        hash ^= bit_keys[y];
                    }
                }
                keys[y][row_value] = hash;
            }
        }

        Self { keys }
    }

    pub(crate) fn hash_board(&self, board: &Board) -> u64 {
        let mut hash = 0u64;

        for y in 0..BOARD_HEIGHT {
            let row = board.rows[y];
            if row != 0 {
                hash ^= self.keys[y][(row as usize) & ROW_MASK];
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
        let size = size.max(1).next_power_of_two();
        debug_assert!(size.is_power_of_two());
        Self {
            entries: vec![TTEntry::default(); size].into_boxed_slice(),
        }
    }

    #[inline]
    fn index(&self, hash: u64) -> usize {
        hash as usize & (self.entries.len() - 1)
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

    fn board_from_rows(rows: [u16; BOARD_HEIGHT]) -> Board {
        let mut board = Board::new();
        for (y, row) in rows.iter().enumerate() {
            board.rows[y] = row & 0x03FF;
            for x in 0..COL_NB {
                if board.rows[y] & (1u16 << x) != 0 {
                    board.cols[x] |= 1u64 << y;
                }
            }
        }
        board
    }

    fn reference_per_bit_hash(board: &Board) -> u64 {
        let mut rng = SplitMix64::new(ZOBRIST_SEED);
        let mut per_bit_keys = [[0u64; BOARD_HEIGHT]; COL_NB];

        for row in per_bit_keys.iter_mut().take(COL_NB) {
            for key in row.iter_mut().take(BOARD_HEIGHT) {
                *key = rng.next_u64();
            }
        }

        let mut hash = 0u64;
        for y in 0..BOARD_HEIGHT {
            let row = board.rows[y];
            for (x, keys) in per_bit_keys.iter().enumerate().take(COL_NB) {
                if row & (1u16 << x) != 0 {
                    hash ^= keys[y];
                }
            }
        }

        hash
    }

    #[test]
    fn row_keyed_hash_matches_per_bit_hash() {
        let keys = ZobristKeys::new();
        assert_eq!(keys.keys.len(), BOARD_HEIGHT);
        assert_eq!(keys.keys[0].len(), 1024);

        let mut boards = vec![
            Board::new(),
            board_with_cells(&[(0, 0), (9, 0), (4, 17), (2, 39)]),
            board_from_rows([0x03FF; BOARD_HEIGHT]),
        ];

        let mut rng = SplitMix64::new(0xD1B5_4A32_D192_ED03);
        for _ in 0..16 {
            let mut rows = [0u16; BOARD_HEIGHT];
            for row in rows.iter_mut() {
                *row = rng.next_u64() as u16 & 0x03FF;
            }
            boards.push(board_from_rows(rows));
        }

        for board in boards {
            assert_eq!(keys.hash_board(&board), reference_per_bit_hash(&board));
        }
    }

    #[test]
    fn index_is_power_of_two_masked() {
        let tt = TranspositionTable::new(1000);
        let len = tt.entries.len();
        assert!(len.is_power_of_two());
        assert_eq!(len, 1024);

        for hash in [0u64, 1, 0x03FF, 0x0400, 0x1234_5678_9ABC_DEF0, u64::MAX] {
            assert_eq!(tt.index(hash), hash as usize & (len - 1));
        }

        let one = TranspositionTable::new(1);
        assert_eq!(one.entries.len(), 1);
        assert_eq!(one.index(0), 0);
        assert_eq!(one.index(u64::MAX), 0);
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
