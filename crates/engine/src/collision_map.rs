//! Precomputed collision maps - single bitcheck instead of 4 mino lookups
//! Cobra-style approach, ported to Rust

use fusion_core::{Board, Piece, Rotation};

/// Per-piece collision lookup - [rot][x] -> u64 of blocked y positions
#[derive(Clone)]
pub struct CollisionMap {
    // [rot][x+2] -> collision bitboard (bit y = blocked)
    map: [[u64; 14]; 4],
}

impl CollisionMap {
    /// Build collision map via column-shift precompute — O(4×14×4) not O(4×14×44×4)
    #[inline]
    pub fn new(board: &Board, piece: Piece) -> Self {
        let mut map = [[0u64; 14]; 4];
        let height_mask: u64 = (1u64 << Board::HEIGHT) - 1;

        for rot in 0..4 {
            let rotation = match rot {
                0 => Rotation::North,
                1 => Rotation::East,
                2 => Rotation::South,
                _ => Rotation::West,
            };
            let minos = piece.minos(rotation);

            for x_offset in 0..14 {
                let x = x_offset as i8 - 2;
                let mut collision_bits = 0u64;

                for &(dx, dy) in &minos {
                    let nx = x + dx;

                    if nx < 0 || nx >= Board::WIDTH as i8 {
                        collision_bits = !0u64;
                        break;
                    }

                    let board_col = board.column(nx as usize) & height_mask;
                    let shifted = if dy > 0 {
                        board_col >> (dy as u32)
                    } else if dy < 0 {
                        board_col << ((-dy) as u32)
                    } else {
                        board_col
                    };
                    collision_bits |= shifted;

                    if dy < 0 {
                        collision_bits |= (1u64 << ((-dy) as u32)) - 1;
                    }
                    let max_y = Board::HEIGHT as i8 - dy;
                    if max_y < 44 && max_y > 0 {
                        collision_bits |= !((1u64 << (max_y as u32)) - 1);
                    } else if max_y <= 0 {
                        collision_bits = !0u64;
                        break;
                    }
                }

                map[rot][x_offset] = collision_bits;
            }
        }

        Self { map }
    }

    /// O(1) collision check - just a bit test
    #[inline(always)]
    pub fn collides(&self, rotation: Rotation, x: i8, y: i8) -> bool {
        let x_idx = (x + 2) as usize;
        if x_idx >= 14 || !(0..44).contains(&y) {
            return true;
        }
        (self.map[rotation as usize][x_idx] & (1u64 << y)) != 0
    }

    /// Raw collision column - for bitboard propagation
    #[inline(always)]
    pub fn get_column(&self, rotation: Rotation, x: i8) -> u64 {
        let x_idx = (x + 2) as usize;
        if x_idx >= 14 {
            return !0u64; // All collide if out of bounds
        }
        self.map[rotation as usize][x_idx]
    }

    /// Inverse - where piece CAN go
    #[inline(always)]
    pub fn get_reachable(&self, rotation: Rotation, x: i8) -> u64 {
        !self.get_column(rotation, x)
    }
}

/// Flood-fill state - tracks reachable positions as bitboards
pub struct ReachabilityMap {
    // [rot][x+2] -> reachable y positions
    reachable: [[u64; 14]; 4],
}

impl ReachabilityMap {
    pub fn new() -> Self {
        Self {
            reachable: [[0u64; 14]; 4],
        }
    }

    /// Softdrop propagation - shift down until collision
    #[inline]
    pub fn propagate_drops(&mut self, collision: &CollisionMap) -> bool {
        let mut changed = false;

        for rot in 0..4 {
            let rotation = match rot {
                0 => Rotation::North,
                1 => Rotation::East,
                2 => Rotation::South,
                _ => Rotation::West,
            };

            for x_idx in 0..14 {
                let x = x_idx as i8 - 2;
                let collision_col = collision.get_column(rotation, x);
                let mut reachable = self.reachable[rot][x_idx];

                // keep shifting down until nothing new
                loop {
                    let new_reach = (reachable >> 1) & !collision_col;
                    let combined = reachable | new_reach;
                    if combined == reachable {
                        break;
                    }
                    reachable = combined;
                    changed = true;
                }

                self.reachable[rot][x_idx] = reachable;
            }
        }

        changed
    }

    #[inline(always)]
    pub fn set_reachable(&mut self, rotation: Rotation, x: i8, y: i8) {
        let x_idx = (x + 2) as usize;
        if x_idx < 14 && (0..44).contains(&y) {
            self.reachable[rotation as usize][x_idx] |= 1u64 << y;
        }
    }

    /// Set entire reachable mask for (rotation, x)
    #[inline(always)]
    pub fn set_reachable_mask(&mut self, rotation: Rotation, x: i8, mask: u64) {
        let x_idx = (x + 2) as usize;
        if x_idx < 14 {
            self.reachable[rotation as usize][x_idx] = mask;
        }
    }

    #[inline(always)]
    pub fn is_reachable(&self, rotation: Rotation, x: i8, y: i8) -> bool {
        let x_idx = (x + 2) as usize;
        if x_idx >= 14 || !(0..44).contains(&y) {
            return false;
        }
        (self.reachable[rotation as usize][x_idx] & (1u64 << y)) != 0
    }

    #[inline(always)]
    pub fn get(&self, rotation: Rotation, x: i8) -> u64 {
        let x_idx = (x + 2) as usize;
        if x_idx >= 14 {
            return 0;
        }
        self.reachable[rotation as usize][x_idx]
    }
}

impl Default for ReachabilityMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collision_map_empty_board() {
        let board = Board::new();
        let cm = CollisionMap::new(&board, Piece::T);

        // T piece at center should not collide high up
        assert!(!cm.collides(Rotation::North, 4, 10));

        // T piece North at y=-1 should collide (floor)
        assert!(cm.collides(Rotation::North, 4, -1));

        // T piece at x=-2 should collide (wall)
        assert!(cm.collides(Rotation::North, -2, 5));
    }

    #[test]
    fn test_collision_map_with_blocks() {
        let mut board = Board::new();
        board.set(4, 5, true);

        let cm = CollisionMap::new(&board, Piece::T);

        // T piece North at (4, 5) should collide - center mino hits block
        assert!(cm.collides(Rotation::North, 4, 5));

        // T piece at (4, 6) should be fine
        assert!(!cm.collides(Rotation::North, 4, 6));
    }

    #[test]
    fn test_reachability_propagation() {
        let board = Board::new();
        let cm = CollisionMap::new(&board, Piece::T);
        let mut reach = ReachabilityMap::new();

        // Start at spawn
        reach.set_reachable(Rotation::North, 4, 20);

        // Propagate drops
        reach.propagate_drops(&cm);

        // Should now be reachable all the way down to floor
        assert!(reach.is_reachable(Rotation::North, 4, 0));
        assert!(reach.is_reachable(Rotation::North, 4, 10));
        assert!(reach.is_reachable(Rotation::North, 4, 20));
    }
}
