//! Cobra-style bitboard flood-fill movegen
//! Uses toSearch/searched separation like Cobra for correct cycle handling
//! Each (rotation, x) pair has a u64 bitboard of y positions

use crate::collision_map::CollisionMap;
use crate::kicks::get_kicks;
use crate::move_list::MoveList;
use crate::movement::detect_all_spin;
use fusion_core::{Board, Move, Piece, Rotation, SpinType};

const SPIN_NONE_IDX: usize = 0;
const SPIN_MINI_IDX: usize = 1;
const SPIN_FULL_IDX: usize = 2;

/// Canonical rotation mapping
/// I/S/Z: 2 shapes (canonicalized with offsets)
/// O: 1 shape (all rotations identical)
/// L/J/T: 4 shapes
const CANONICAL_ROT: [[Rotation; 4]; 7] = [
    [
        Rotation::North,
        Rotation::East,
        Rotation::North,
        Rotation::East,
    ],
    // O: 1 shape
    [
        Rotation::North,
        Rotation::North,
        Rotation::North,
        Rotation::North,
    ],
    // T: 4 shapes
    [
        Rotation::North,
        Rotation::East,
        Rotation::South,
        Rotation::West,
    ],
    // S: 2 shapes
    [
        Rotation::North,
        Rotation::East,
        Rotation::North,
        Rotation::East,
    ],
    // Z: 2 shapes
    [
        Rotation::North,
        Rotation::East,
        Rotation::North,
        Rotation::East,
    ],
    // J: 4 shapes
    [
        Rotation::North,
        Rotation::East,
        Rotation::South,
        Rotation::West,
    ],
    // L: 4 shapes
    [
        Rotation::North,
        Rotation::East,
        Rotation::South,
        Rotation::West,
    ],
];

/// Canonical offset for symmetric rotations (Fusion minos)
#[inline(always)]
fn canonical_offset(piece: Piece, rotation: Rotation) -> (i8, i8) {
    match piece {
        Piece::S | Piece::Z => match rotation {
            Rotation::South => (0, -1),
            Rotation::West => (-1, 0),
            _ => (0, 0),
        },
        Piece::I => match rotation {
            Rotation::South => (-1, 0),
            Rotation::West => (0, 1),
            _ => (0, 0),
        },
        _ => (0, 0),
    }
}

/// Check if rotation is canonical for this piece
/// O: only North, I/S/Z: North or East, L/J/T: all 4
#[allow(dead_code)]
#[inline(always)]
fn is_canonical_rotation(piece: Piece, rotation: Rotation) -> bool {
    match piece {
        Piece::O => rotation == Rotation::North,
        Piece::I | Piece::S | Piece::Z => rotation == Rotation::North || rotation == Rotation::East,
        _ => true, // L/J/T: all rotations are unique
    }
}

#[inline(always)]
fn canonical_rotation(piece: Piece, rotation: Rotation) -> Rotation {
    CANONICAL_ROT[piece as usize][rotation as usize]
}

const HEIGHT_MASK: u64 = (1u64 << 44) - 1;

#[inline(always)]
fn shift_y(mask: u64, dy: i8) -> u64 {
    if dy > 0 {
        (mask << (dy as u32)) & HEIGHT_MASK
    } else if dy < 0 {
        mask >> ((-dy) as u32)
    } else {
        mask
    }
}

#[inline]
fn seed_initial_states(
    to_search: &mut [[u64; 14]; 4],
    remaining: &mut u64,
    collision: &CollisionMap,
    piece: Piece,
) -> bool {
    let spawn_x = piece.spawn_x();
    let spawn_y = piece.spawn_y();
    let spawn_x_idx = (spawn_x + 2) as usize;

    if !collision.collides(Rotation::North, spawn_x, spawn_y) {
        let spawn_bit = 1u64 << spawn_y;
        to_search[0][spawn_x_idx] = spawn_bit;
        *remaining |= 1u64 << (spawn_x_idx * 4);
        return true;
    }

    false
}

#[inline]
fn classify_t_spin_bits(
    board: &Board,
    to_rot: Rotation,
    target_x: i8,
    bits: u64,
    kick_idx: usize,
) -> (u64, u64, u64) {
    let left = if target_x > 0 {
        board.column((target_x - 1) as usize)
    } else {
        !0u64
    };
    let right = if target_x < Board::WIDTH as i8 - 1 {
        board.column((target_x + 1) as usize)
    } else {
        !0u64
    };

    // Corner masks follow Cobra ordering:
    // 0=NW, 1=NE, 2=SE, 3=SW
    let corners = [left >> 1, right >> 1, (right << 1) | 1, (left << 1) | 1];

    let spins = bits
        & ((corners[0] & corners[1] & (corners[2] | corners[3]))
            | (corners[2] & corners[3] & (corners[0] | corners[1])));

    let no_spin_bits = bits & !spins;
    if spins == 0 {
        return (no_spin_bits, 0, 0);
    }

    let r = to_rot as usize;
    let full_mask = spins & corners[r] & corners[(r + 1) & 3];

    if kick_idx >= 4 {
        (no_spin_bits, 0, spins)
    } else {
        let full_bits = full_mask;
        let mini_bits = spins & !full_mask;
        (no_spin_bits, mini_bits, full_bits)
    }
}

/// Cobra-style movegen — dispatches to T-piece (spin tracking) or non-T (lean) path
#[inline]
pub fn generate_moves_bitboard(board: &Board, piece: Piece) -> MoveList {
    if piece == Piece::T {
        generate_moves_t(board)
    } else {
        generate_moves_no_spin(board, piece)
    }
}

/// Non-T fast path: zero spin tracking overhead
#[inline]
fn generate_moves_no_spin(board: &Board, piece: Piece) -> MoveList {
    let collision = CollisionMap::new(board, piece);

    let mut to_search = [[0u64; 14]; 4];
    let mut searched = [[0u64; 14]; 4];

    for rot in 0..4 {
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];
        for x_idx in 0..14 {
            searched[rot][x_idx] = collision.get_column(rotation, x_idx as i8 - 2);
        }
    }

    let mut remaining: u64 = 0;
    if !seed_initial_states(&mut to_search, &mut remaining, &collision, piece) {
        return MoveList::new();
    }

    let mut move_set = [[0u64; 14]; 4];

    while remaining != 0 {
        let index = remaining.trailing_zeros() as usize;
        let x_idx = index / 4;
        let rot = index % 4;
        let x = x_idx as i8 - 2;
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];

        let mut current = to_search[rot][x_idx];
        if current == 0 {
            remaining &= !(1u64 << index);
            continue;
        }

        let blocked = collision.get_column(rotation, x);

        let mut m = (current >> 1) & !blocked & HEIGHT_MASK;
        while (m & current) != m {
            current |= m;
            m |= (m >> 1) & !blocked & HEIGHT_MASK;
        }
        to_search[rot][x_idx] = current;

        let lock_mask = (blocked << 1) | 1;
        let locking = current & lock_mask & !blocked;
        move_set[rot][x_idx] |= locking;

        if x > -2 {
            let left_x_idx = x_idx - 1;
            let left_blocked = collision.get_column(rotation, x - 1);
            let projected = current & !left_blocked;
            let new_bits = projected & !searched[rot][left_x_idx];
            if new_bits != 0 {
                to_search[rot][left_x_idx] |= new_bits;
                remaining |= 1u64 << (left_x_idx * 4 + rot);
            }
        }

        if x < 11 {
            let right_x_idx = x_idx + 1;
            let right_blocked = collision.get_column(rotation, x + 1);
            let projected = current & !right_blocked;
            let new_bits = projected & !searched[rot][right_x_idx];
            if new_bits != 0 {
                to_search[rot][right_x_idx] |= new_bits;
                remaining |= 1u64 << (right_x_idx * 4 + rot);
            }
        }

        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.cw(),
            x_idx,
            current,
            board,
            None,
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.ccw(),
            x_idx,
            current,
            board,
            None,
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.flip(),
            x_idx,
            current,
            board,
            None,
            &mut remaining,
            piece != Piece::I,
        );

        searched[rot][x_idx] |= to_search[rot][x_idx];
        to_search[rot][x_idx] = 0;
        remaining &= !(1u64 << index);
    }

    extract_placements_cobra(board, piece, &move_set, &collision, None)
}

/// T-piece path: full spin tracking (spin_set + variant emission)
#[inline]
fn generate_moves_t(board: &Board) -> MoveList {
    let piece = Piece::T;
    let collision = CollisionMap::new(board, piece);

    let mut to_search = [[0u64; 14]; 4];
    let mut searched = [[0u64; 14]; 4];

    for rot in 0..4 {
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];
        for x_idx in 0..14 {
            searched[rot][x_idx] = collision.get_column(rotation, x_idx as i8 - 2);
        }
    }

    let mut remaining: u64 = 0;
    if !seed_initial_states(&mut to_search, &mut remaining, &collision, piece) {
        return MoveList::new();
    }

    let mut move_set = [[0u64; 14]; 4];
    let mut spin_set = [[[0u64; 14]; 4]; 3];
    for x_idx in 0..14 {
        spin_set[SPIN_NONE_IDX][0][x_idx] |= to_search[0][x_idx];
    }

    while remaining != 0 {
        let index = remaining.trailing_zeros() as usize;
        let x_idx = index / 4;
        let rot = index % 4;
        let x = x_idx as i8 - 2;
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];

        let mut current = to_search[rot][x_idx];
        if current == 0 {
            remaining &= !(1u64 << index);
            continue;
        }

        let blocked = collision.get_column(rotation, x);

        let mut m = (current >> 1) & !blocked & HEIGHT_MASK;
        while (m & current) != m {
            current |= m;
            m |= (m >> 1) & !blocked & HEIGHT_MASK;
        }
        spin_set[SPIN_NONE_IDX][rot][x_idx] |= m;
        to_search[rot][x_idx] = current;

        let lock_mask = (blocked << 1) | 1;
        let locking = current & lock_mask & !blocked;
        move_set[rot][x_idx] |= locking;

        if x > -2 {
            let left_x_idx = x_idx - 1;
            let left_blocked = collision.get_column(rotation, x - 1);
            let projected = current & !left_blocked;
            let new_bits = projected & !searched[rot][left_x_idx];
            if new_bits != 0 {
                to_search[rot][left_x_idx] |= new_bits;
                remaining |= 1u64 << (left_x_idx * 4 + rot);
                spin_set[SPIN_NONE_IDX][rot][left_x_idx] |= new_bits;
            }
        }

        if x < 11 {
            let right_x_idx = x_idx + 1;
            let right_blocked = collision.get_column(rotation, x + 1);
            let projected = current & !right_blocked;
            let new_bits = projected & !searched[rot][right_x_idx];
            if new_bits != 0 {
                to_search[rot][right_x_idx] |= new_bits;
                remaining |= 1u64 << (right_x_idx * 4 + rot);
                spin_set[SPIN_NONE_IDX][rot][right_x_idx] |= new_bits;
            }
        }

        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.cw(),
            x_idx,
            current,
            board,
            Some(&mut spin_set),
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.ccw(),
            x_idx,
            current,
            board,
            Some(&mut spin_set),
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.flip(),
            x_idx,
            current,
            board,
            Some(&mut spin_set),
            &mut remaining,
            piece != Piece::I,
        );

        searched[rot][x_idx] |= to_search[rot][x_idx];
        to_search[rot][x_idx] = 0;
        remaining &= !(1u64 << index);
    }

    extract_placements_cobra(board, piece, &move_set, &collision, Some(&spin_set))
}

/// Generate moves with spin detection disabled.
/// Placement set is identical to generate_moves_bitboard(); only spin_type differs.
pub fn generate_moves_bitboard_no_spin(board: &Board, piece: Piece) -> MoveList {
    let moves = generate_moves_bitboard(board, piece);
    let mut no_spin_moves = MoveList::new();
    let mut seen = [[[false; 44]; 14]; 4];

    for mv in moves.iter() {
        let rot = mv.rotation as usize;
        let x_idx = mv.x + 2;
        let y_idx = mv.y;

        if (0..14).contains(&x_idx) && (0..44).contains(&y_idx) {
            let xi = x_idx as usize;
            let yi = y_idx as usize;
            if seen[rot][xi][yi] {
                continue;
            }
            seen[rot][xi][yi] = true;
        }

        no_spin_moves.push(Move {
            spin_type: SpinType::None,
            ..*mv
        });
    }

    no_spin_moves
}

/// Allocation-free move count — lean non-T fast path, full spin tracking for T only.
/// Skips spin_set allocation (1,344 bytes) and all spin branches for 6/7 pieces.
#[inline]
pub fn count_placements_cobra(board: &Board, piece: Piece) -> usize {
    if piece == Piece::T {
        count_placements_t(board)
    } else {
        count_placements_no_spin(board, piece)
    }
}

/// Non-T fast path: zero spin tracking, pure BFS + canonical dedup + popcount
#[inline]
fn count_placements_no_spin(board: &Board, piece: Piece) -> usize {
    let collision = CollisionMap::new(board, piece);

    let mut to_search = [[0u64; 14]; 4];
    let mut searched = [[0u64; 14]; 4];

    for rot in 0..4 {
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];
        for x_idx in 0..14 {
            let x = x_idx as i8 - 2;
            searched[rot][x_idx] = collision.get_column(rotation, x);
        }
    }

    let mut remaining: u64 = 0;
    if !seed_initial_states(&mut to_search, &mut remaining, &collision, piece) {
        return 0;
    }

    let mut move_set = [[0u64; 14]; 4];

    while remaining != 0 {
        let index = remaining.trailing_zeros() as usize;
        let x_idx = index / 4;
        let rot = index % 4;
        let x = x_idx as i8 - 2;
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];

        let mut current = to_search[rot][x_idx];
        if current == 0 {
            remaining &= !(1u64 << index);
            continue;
        }

        let blocked = collision.get_column(rotation, x);

        // Softdrop to fixpoint — no spin tracking
        let mut m = (current >> 1) & !blocked & HEIGHT_MASK;
        while (m & current) != m {
            current |= m;
            m |= (m >> 1) & !blocked & HEIGHT_MASK;
        }
        to_search[rot][x_idx] = current;

        let lock_mask = (blocked << 1) | 1;
        let locking = current & lock_mask & !blocked;
        move_set[rot][x_idx] |= locking;

        if x > -2 {
            let left_x_idx = x_idx - 1;
            let left_blocked = collision.get_column(rotation, x - 1);
            let projected = current & !left_blocked;
            let new_bits = projected & !searched[rot][left_x_idx];
            if new_bits != 0 {
                to_search[rot][left_x_idx] |= new_bits;
                remaining |= 1u64 << (left_x_idx * 4 + rot);
            }
        }

        if x < 11 {
            let right_x_idx = x_idx + 1;
            let right_blocked = collision.get_column(rotation, x + 1);
            let projected = current & !right_blocked;
            let new_bits = projected & !searched[rot][right_x_idx];
            if new_bits != 0 {
                to_search[rot][right_x_idx] |= new_bits;
                remaining |= 1u64 << (right_x_idx * 4 + rot);
            }
        }

        // Rotations — pass None for spin_set (inlined, dead-code eliminated)
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.cw(),
            x_idx,
            current,
            board,
            None,
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.ccw(),
            x_idx,
            current,
            board,
            None,
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.flip(),
            x_idx,
            current,
            board,
            None,
            &mut remaining,
            piece != Piece::I,
        );

        searched[rot][x_idx] |= to_search[rot][x_idx];
        to_search[rot][x_idx] = 0;
        remaining &= !(1u64 << index);
    }

    // Count: canonical dedup + popcount (no spin variant expansion)
    let mut seen = [[0u64; 16]; 4];
    let mut count = 0usize;

    for rot in 0..4 {
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];

        let canon_rot = canonical_rotation(piece, rotation);
        let (off_x, off_y) = canonical_offset(piece, rotation);
        let canon_rot_idx = canon_rot as usize;

        for x_idx in 0..14 {
            let locked = move_set[rot][x_idx];
            if locked == 0 {
                continue;
            }

            let x = x_idx as i8 - 2;
            let canon_x = x + off_x;
            if canon_x < -2 || canon_x > 12 {
                continue;
            }
            let canon_x_idx = (canon_x + 2) as usize;

            let shifted = if off_y > 0 {
                locked << (off_y as u32)
            } else if off_y < 0 {
                locked >> ((-off_y) as u32)
            } else {
                locked
            };

            let new_bits = shifted & !seen[canon_rot_idx][canon_x_idx];
            seen[canon_rot_idx][canon_x_idx] |= shifted;
            count += new_bits.count_ones() as usize;
        }
    }

    count
}

/// T-piece count path: full spin tracking (spin_set allocation + variant counting)
#[inline]
fn count_placements_t(board: &Board) -> usize {
    let piece = Piece::T;
    let collision = CollisionMap::new(board, piece);

    let mut to_search = [[0u64; 14]; 4];
    let mut searched = [[0u64; 14]; 4];

    for rot in 0..4 {
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];
        for x_idx in 0..14 {
            let x = x_idx as i8 - 2;
            searched[rot][x_idx] = collision.get_column(rotation, x);
        }
    }

    let mut remaining: u64 = 0;
    if !seed_initial_states(&mut to_search, &mut remaining, &collision, piece) {
        return 0;
    }

    let mut move_set = [[0u64; 14]; 4];
    let mut spin_set = [[[0u64; 14]; 4]; 3];
    for x_idx in 0..14 {
        spin_set[SPIN_NONE_IDX][0][x_idx] |= to_search[0][x_idx];
    }

    while remaining != 0 {
        let index = remaining.trailing_zeros() as usize;
        let x_idx = index / 4;
        let rot = index % 4;
        let x = x_idx as i8 - 2;
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];

        let mut current = to_search[rot][x_idx];
        if current == 0 {
            remaining &= !(1u64 << index);
            continue;
        }

        let blocked = collision.get_column(rotation, x);

        // Softdrop to fixpoint — NONE spin for drop closure
        let mut m = (current >> 1) & !blocked & HEIGHT_MASK;
        while (m & current) != m {
            current |= m;
            m |= (m >> 1) & !blocked & HEIGHT_MASK;
        }
        spin_set[SPIN_NONE_IDX][rot][x_idx] |= m;
        to_search[rot][x_idx] = current;

        // Lock detection
        let lock_mask = (blocked << 1) | 1;
        let locking = current & lock_mask & !blocked;
        move_set[rot][x_idx] |= locking;

        // Shift left — NONE spin
        if x > -2 {
            let left_x_idx = x_idx - 1;
            let left_blocked = collision.get_column(rotation, x - 1);
            let projected = current & !left_blocked;
            let new_bits = projected & !searched[rot][left_x_idx];
            if new_bits != 0 {
                to_search[rot][left_x_idx] |= new_bits;
                remaining |= 1u64 << (left_x_idx * 4 + rot);
                spin_set[SPIN_NONE_IDX][rot][left_x_idx] |= new_bits;
            }
        }

        // Shift right — NONE spin
        if x < 11 {
            let right_x_idx = x_idx + 1;
            let right_blocked = collision.get_column(rotation, x + 1);
            let projected = current & !right_blocked;
            let new_bits = projected & !searched[rot][right_x_idx];
            if new_bits != 0 {
                to_search[rot][right_x_idx] |= new_bits;
                remaining |= 1u64 << (right_x_idx * 4 + rot);
                spin_set[SPIN_NONE_IDX][rot][right_x_idx] |= new_bits;
            }
        }

        // Rotations — full spin tracking
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.cw(),
            x_idx,
            current,
            board,
            Some(&mut spin_set),
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.ccw(),
            x_idx,
            current,
            board,
            Some(&mut spin_set),
            &mut remaining,
            piece != Piece::I,
        );
        propagate_rotation_cobra(
            &mut to_search,
            &searched,
            &collision,
            piece,
            rotation,
            rotation.flip(),
            x_idx,
            current,
            board,
            Some(&mut spin_set),
            &mut remaining,
            piece != Piece::I,
        );

        searched[rot][x_idx] |= to_search[rot][x_idx];
        to_search[rot][x_idx] = 0;
        remaining &= !(1u64 << index);
    }

    // Count with spin variant expansion
    let mut seen = [[0u64; 16]; 4];
    let mut count = 0usize;

    for rot in 0..4 {
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];

        let canon_rot = canonical_rotation(piece, rotation);
        let (off_x, off_y) = canonical_offset(piece, rotation);
        let canon_rot_idx = canon_rot as usize;

        for x_idx in 0..14 {
            let locked = move_set[rot][x_idx];
            if locked == 0 {
                continue;
            }

            let x = x_idx as i8 - 2;
            let canon_x = x + off_x;
            if canon_x < -2 || canon_x > 12 {
                continue;
            }
            let canon_x_idx = (canon_x + 2) as usize;

            let shifted = if off_y > 0 {
                locked << (off_y as u32)
            } else if off_y < 0 {
                locked >> ((-off_y) as u32)
            } else {
                locked
            };

            let new_bits = shifted & !seen[canon_rot_idx][canon_x_idx];
            seen[canon_rot_idx][canon_x_idx] |= shifted;

            // T-piece: count each spin variant separately
            let mut bits = new_bits;
            while bits != 0 {
                let canon_y = bits.trailing_zeros() as u32;
                bits &= bits - 1;
                let y = (canon_y as i8 - off_y) as u32;
                let bit = 1u64 << y;
                if (spin_set[SPIN_NONE_IDX][rot][x_idx] & bit) != 0 {
                    count += 1;
                }
                if (spin_set[SPIN_MINI_IDX][rot][x_idx] & bit) != 0 {
                    count += 1;
                }
                if (spin_set[SPIN_FULL_IDX][rot][x_idx] & bit) != 0 {
                    count += 1;
                }
            }
        }
    }

    count
}

/// Count moves without constructing Move structs
/// Optimization for perft depth-1
#[inline]
pub fn count_moves_bitboard(board: &Board, piece: Piece) -> usize {
    count_placements_cobra(board, piece)
}

/// Propagate rotation with kicks - Cobra-style source subtraction
/// Kicks must be applied in table order (first-valid semantics)
#[inline(always)]
fn propagate_rotation_cobra(
    to_search: &mut [[u64; 14]; 4],
    searched: &[[u64; 14]; 4],
    collision: &CollisionMap,
    piece: Piece,
    from_rot: Rotation,
    to_rot: Rotation,
    src_x_idx: usize,
    source: u64,
    board: &Board,
    mut spin_set_t: Option<&mut [[[u64; 14]; 4]; 3]>,
    remaining: &mut u64,
    source_subtract: bool,
) {
    let to_rot_idx = to_rot as usize;
    let src_x = src_x_idx as i8 - 2;

    let mut current = source & HEIGHT_MASK;
    if current == 0 {
        return;
    }

    // Get kicks and apply in table order (first-valid semantics)
    let kicks = get_kicks(piece, from_rot, to_rot);
    for (kick_idx, &(kick_x, kick_y)) in kicks.iter().enumerate() {
        let target_x = src_x + kick_x;
        let target_x_idx = (target_x + 2) as usize;

        if target_x_idx >= 14 {
            continue;
        }

        let target_blocked = collision.get_column(to_rot, target_x);

        // Project source positions by kick offset
        let projected = shift_y(current, kick_y);
        let valid = projected & !target_blocked & HEIGHT_MASK;

        if piece == Piece::T && valid != 0 {
            if let Some(spin_set) = spin_set_t.as_deref_mut() {
                let (none_bits, mini_bits, full_bits) =
                    classify_t_spin_bits(board, to_rot, target_x, valid, kick_idx);
                spin_set[SPIN_NONE_IDX][to_rot_idx][target_x_idx] |= none_bits;
                spin_set[SPIN_MINI_IDX][to_rot_idx][target_x_idx] |= mini_bits;
                spin_set[SPIN_FULL_IDX][to_rot_idx][target_x_idx] |= full_bits;
            }
        }

        let new_bits = valid & !searched[to_rot_idx][target_x_idx];
        if new_bits != 0 {
            to_search[to_rot_idx][target_x_idx] |= new_bits;
            *remaining |= 1u64 << (target_x_idx * 4 + to_rot_idx);
        }

        // Source subtraction: back-project valid positions and remove from current
        if source_subtract {
            let satisfied = shift_y(valid, -kick_y);
            current &= !satisfied;
        }

        if current == 0 {
            break;
        }
    }
}

/// Extract final placements from move_set
/// Explores ALL rotations but deduplicates using canonical coordinates
/// OUTPUT uses CANONICAL coordinates and rotation
fn extract_placements_cobra(
    board: &Board,
    piece: Piece,
    move_set: &[[u64; 14]; 4],
    _collision: &CollisionMap,
    spin_set_t: Option<&[[[u64; 14]; 4]; 3]>,
) -> MoveList {
    let mut moves = MoveList::new();
    let mut seen = [[0u64; 16]; 4];

    for rot in 0..4 {
        let rotation = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ][rot];

        let canon_rot = canonical_rotation(piece, rotation);
        let (off_x, off_y) = canonical_offset(piece, rotation);
        let canon_rot_idx = canon_rot as usize;

        for x_idx in 0..14 {
            let x = x_idx as i8 - 2;
            let locked = move_set[rot][x_idx];
            if locked == 0 {
                continue;
            }

            let canon_x = x + off_x;
            if canon_x < -2 || canon_x > 12 {
                continue;
            }
            let canon_x_idx = (canon_x + 2) as usize;

            let mut bits = locked;
            while bits != 0 {
                let y = bits.trailing_zeros() as i8;
                bits &= bits - 1;

                let canon_y = y + off_y;
                if canon_y < 0 || canon_y >= 64 {
                    continue;
                }

                let canon_y_bit = 1u64 << (canon_y as u32);
                if (seen[canon_rot_idx][canon_x_idx] & canon_y_bit) != 0 {
                    continue;
                }
                seen[canon_rot_idx][canon_x_idx] |= canon_y_bit;

                if let Some(spin_set) = spin_set_t {
                    let bit = 1u64 << (y as u32);
                    let has_none = (spin_set[SPIN_NONE_IDX][rot][x_idx] & bit) != 0;
                    let has_mini = (spin_set[SPIN_MINI_IDX][rot][x_idx] & bit) != 0;
                    let has_full = (spin_set[SPIN_FULL_IDX][rot][x_idx] & bit) != 0;

                    if has_none {
                        moves.push(Move {
                            piece,
                            rotation: canon_rot,
                            x: canon_x,
                            y: canon_y,
                            hold_used: false,
                            spin_type: SpinType::None,
                        });
                    }
                    if has_mini {
                        moves.push(Move {
                            piece,
                            rotation: canon_rot,
                            x: canon_x,
                            y: canon_y,
                            hold_used: false,
                            spin_type: SpinType::Mini,
                        });
                    }
                    if has_full {
                        moves.push(Move {
                            piece,
                            rotation: canon_rot,
                            x: canon_x,
                            y: canon_y,
                            hold_used: false,
                            spin_type: SpinType::Full,
                        });
                    }

                    if !has_none && !has_mini && !has_full {
                        let spin_type = detect_all_spin(board, piece, canon_x, canon_y, canon_rot);
                        moves.push(Move {
                            piece,
                            rotation: canon_rot,
                            x: canon_x,
                            y: canon_y,
                            hold_used: false,
                            spin_type,
                        });
                    }
                } else {
                    let spin_type = detect_all_spin(board, piece, canon_x, canon_y, canon_rot);
                    moves.push(Move {
                        piece,
                        rotation: canon_rot,
                        x: canon_x,
                        y: canon_y,
                        hold_used: false,
                        spin_type,
                    });
                }
            }
        }
    }

    moves
}

/// Find landing y given current y and collision bitboard
#[allow(dead_code)]
#[inline]
fn find_landing(from_y: i8, blocked: u64) -> i8 {
    let below_mask = (1u64 << from_y) - 1;
    let blocked_below = blocked & below_mask;

    if blocked_below == 0 {
        0
    } else {
        (64 - blocked_below.leading_zeros()) as i8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::apply_move_mut;
    use crate::collision::can_place;
    use crate::kicks::get_kicks;

    fn board_from_fixture_rows(rows: &[&str; 40], reverse_x: bool, reverse_y: bool) -> Board {
        let mut board = Board::new();
        for (src_y, row) in rows.iter().enumerate() {
            let y = if reverse_y { 39 - src_y } else { src_y };
            for (i, ch) in row.chars().enumerate() {
                if ch == '1' {
                    let x = if reverse_x { 9 - i } else { i };
                    board.set(x, y, true);
                }
            }
        }
        board
    }

    fn board_equals_fixture_rows(
        board: &Board,
        rows: &[&str; 40],
        reverse_x: bool,
        reverse_y: bool,
    ) -> bool {
        for (src_y, row) in rows.iter().enumerate() {
            let y = if reverse_y { 39 - src_y } else { src_y };
            for (i, ch) in row.chars().enumerate() {
                let x = if reverse_x { 9 - i } else { i };
                let filled = ch == '1';
                if board.get(x, y) != filled {
                    return false;
                }
            }
        }
        true
    }

    fn first_row_mismatch(
        board: &Board,
        rows: &[&str; 40],
        reverse_x: bool,
        reverse_y: bool,
    ) -> Option<usize> {
        for (src_y, row) in rows.iter().enumerate() {
            let y = if reverse_y { 39 - src_y } else { src_y };
            for (i, ch) in row.chars().enumerate() {
                let x = if reverse_x { 9 - i } else { i };
                let filled = ch == '1';
                if board.get(x, y) != filled {
                    return Some(src_y);
                }
            }
        }
        None
    }

    fn board_row_to_fixture_string(
        board: &Board,
        src_y: usize,
        reverse_x: bool,
        reverse_y: bool,
    ) -> String {
        let y = if reverse_y { 39 - src_y } else { src_y };
        let mut row = String::with_capacity(10);
        for i in 0..10 {
            let x = if reverse_x { 9 - i } else { i };
            row.push(if board.get(x, y) { '1' } else { '0' });
        }
        row
    }

    fn cell_diff_count(
        board: &Board,
        rows: &[&str; 40],
        reverse_x: bool,
        reverse_y: bool,
    ) -> usize {
        let mut diff = 0usize;
        for (src_y, row) in rows.iter().enumerate() {
            let y = if reverse_y { 39 - src_y } else { src_y };
            for (i, ch) in row.chars().enumerate() {
                let x = if reverse_x { 9 - i } else { i };
                let filled = ch == '1';
                if board.get(x, y) != filled {
                    diff += 1;
                }
            }
        }
        diff
    }

    fn added_cells_between(before: &Board, after: &Board) -> Vec<(usize, usize)> {
        let mut cells = Vec::new();
        for x in 0..10 {
            for y in 0..40 {
                if !before.get(x, y) && after.get(x, y) {
                    cells.push((x, y));
                }
            }
        }
        cells
    }

    fn dump_i_kick_attempts(
        board: &Board,
        from: Rotation,
        x: i8,
        y: i8,
        to: Rotation,
        label: &str,
    ) {
        let kicks = get_kicks(Piece::I, from, to);
        eprintln!(
            "kick-debug {} from={:?}@({}, {}) -> {:?}",
            label, from, x, y, to
        );
        for (idx, (kx, ky)) in kicks.iter().enumerate() {
            let tx = x + *kx;
            let ty = y + *ky;
            let legal = can_place(board, Piece::I, to, tx, ty);
            let can_fall = legal && ty > 0 && can_place(board, Piece::I, to, tx, ty - 1);
            eprintln!(
                "  kick{} off=({}, {}) target=({}, {}) legal={} can_fall={}",
                idx, kx, ky, tx, ty, legal, can_fall
            );
        }
    }

    fn first_legal_i_kick(
        board: &Board,
        from: Rotation,
        x: i8,
        y: i8,
        to: Rotation,
    ) -> Option<(usize, i8, i8, bool)> {
        let kicks = get_kicks(Piece::I, from, to);
        for (idx, (kx, ky)) in kicks.iter().enumerate() {
            let tx = x + *kx;
            let ty = y + *ky;
            if can_place(board, Piece::I, to, tx, ty) {
                let can_fall = ty > 0 && can_place(board, Piece::I, to, tx, ty - 1);
                return Some((idx, tx, ty, can_fall));
            }
        }
        None
    }

    #[test]
    fn test_bitboard_movegen_empty_board() {
        let board = Board::new();
        let moves = generate_moves_bitboard(&board, Piece::T);
        assert!(!moves.is_empty());
    }

    #[test]
    fn test_spawn_blocked_returns_no_moves_t() {
        let piece = Piece::T;
        let mut board = Board::new();
        let spawn_x = piece.spawn_x();
        let spawn_y = piece.spawn_y();

        for (dx, dy) in piece.minos(Rotation::North) {
            board.set((spawn_x + dx) as usize, (spawn_y + dy) as usize, true);
        }

        let moves = generate_moves_bitboard(&board, piece);
        assert!(moves.is_empty());
        assert_eq!(count_placements_cobra(&board, piece), 0);
    }

    #[test]
    fn test_spawn_blocked_returns_no_moves_i() {
        let piece = Piece::I;
        let mut board = Board::new();
        let spawn_x = piece.spawn_x();
        let spawn_y = piece.spawn_y();

        for (dx, dy) in piece.minos(Rotation::North) {
            board.set((spawn_x + dx) as usize, (spawn_y + dy) as usize, true);
        }

        let moves = generate_moves_bitboard(&board, piece);
        assert!(moves.is_empty());
        assert_eq!(count_placements_cobra(&board, piece), 0);
    }

    #[test]
    fn test_bitboard_matches_cobra_reference() {
        // Cobra reference values for empty board
        let expected: [(Piece, usize); 7] = [
            (Piece::I, 17),
            (Piece::O, 9),
            (Piece::T, 34),
            (Piece::S, 17),
            (Piece::Z, 17),
            (Piece::J, 34),
            (Piece::L, 34),
        ];
        let board = Board::new();

        for (piece, cobra_count) in expected {
            let bitboard = generate_moves_bitboard(&board, piece);

            assert_eq!(
                bitboard.len(),
                cobra_count,
                "Piece {:?}: bitboard={} cobra={}",
                piece,
                bitboard.len(),
                cobra_count
            );
        }
    }

    #[test]
    fn test_bitboard_with_blocks() {
        let mut board = Board::new();
        for x in 0..10 {
            board.set(x, 0, true);
        }
        board.set(5, 0, false);

        for piece in [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::S,
            Piece::Z,
            Piece::J,
            Piece::L,
        ] {
            let bitboard = generate_moves_bitboard(&board, piece);

            // With blocks, should have moves (may be fewer than empty board)
            assert!(
                !bitboard.is_empty(),
                "With blocks - Piece {:?} should have moves",
                piece
            );
        }
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_lock386_r4_fixture_reachability_debug() {
        let before_rows = [
            "1111111101",
            "1111101111",
            "1111101111",
            "1111111011",
            "1111111011",
            "1110111111",
            "1110111111",
            "1110111111",
            "1110111111",
            "1110111111",
            "1110000011",
            "1111000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        let expected_after_rows = [
            "1111111101",
            "1111101111",
            "1111101111",
            "1111111011",
            "1111111011",
            "1111111111",
            "1111111111",
            "1111111111",
            "1111111111",
            "1110111111",
            "1110000011",
            "1111000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        for (reverse_x, reverse_y) in [(false, false), (true, false), (false, true), (true, true)] {
            let board = board_from_fixture_rows(&before_rows, reverse_x, reverse_y);
            let expected_after_board =
                board_from_fixture_rows(&expected_after_rows, reverse_x, reverse_y);
            let expected_added = added_cells_between(&board, &expected_after_board);
            let moves = generate_moves_bitboard(&board, Piece::I);

            let mut board_match_count = 0usize;
            let mut first_match: Option<(Rotation, i8, i8, SpinType)> = None;
            let mut best_diff = usize::MAX;
            let mut best_move: Option<(Rotation, i8, i8, SpinType)> = None;
            let mut top: Vec<(usize, Rotation, i8, i8, SpinType)> = Vec::new();
            let mut rot_counts = [0usize; 4];
            let mut rot_best = [usize::MAX; 4];
            let mut best_added_overlap = 0usize;
            let mut best_added_move: Option<(Rotation, i8, i8, SpinType)> = None;
            let mut floating_count = 0usize;
            for m in &moves {
                let mut trial = board.clone();
                apply_move_mut(&mut trial, m);
                let diff = cell_diff_count(&trial, &expected_after_rows, reverse_x, reverse_y);
                if diff < best_diff {
                    best_diff = diff;
                    best_move = Some((m.rotation, m.x, m.y, m.spin_type));
                }
                let rot_idx = m.rotation as usize;
                if diff < rot_best[rot_idx] {
                    rot_best[rot_idx] = diff;
                }
                if board_equals_fixture_rows(&trial, &expected_after_rows, reverse_x, reverse_y) {
                    board_match_count += 1;
                    if first_match.is_none() {
                        first_match = Some((m.rotation, m.x, m.y, m.spin_type));
                    }
                }
                top.push((diff, m.rotation, m.x, m.y, m.spin_type));
                rot_counts[m.rotation as usize] += 1;

                if m.y > 0 && can_place(&board, m.piece, m.rotation, m.x, m.y - 1) {
                    floating_count += 1;
                }

                let candidate_added = added_cells_between(&board, &trial);
                let overlap = candidate_added
                    .iter()
                    .filter(|cell| expected_added.contains(cell))
                    .count();
                if overlap > best_added_overlap {
                    best_added_overlap = overlap;
                    best_added_move = Some((m.rotation, m.x, m.y, m.spin_type));
                }
            }
            top.sort_by_key(|entry| entry.0);

            let has_expected = moves.iter().any(|m| {
                m.piece == Piece::I
                    && m.rotation == Rotation::North
                    && m.x == 1
                    && m.y == 11
                    && m.spin_type == SpinType::None
            });

            let mut expected_mismatch = None;
            if let Some(mv) = moves.iter().find(|m| {
                m.piece == Piece::I
                    && m.rotation == Rotation::North
                    && m.x == 1
                    && m.y == 11
                    && m.spin_type == SpinType::None
            }) {
                let mut trial = board.clone();
                apply_move_mut(&mut trial, mv);
                expected_mismatch =
                    first_row_mismatch(&trial, &expected_after_rows, reverse_x, reverse_y);
                if let Some(src_y) = expected_mismatch {
                    let actual_row =
                        board_row_to_fixture_string(&trial, src_y, reverse_x, reverse_y);
                    eprintln!(
                        "mismatch row {}: exp={} act={}",
                        src_y, expected_after_rows[src_y], actual_row
                    );
                }
            }

            eprintln!(
                "fixture I_lock386_r4 rx={} ry={}: candidates={}, has_expected={}, board_match_count={}, first_match={:?}",
                reverse_x,
                reverse_y,
                moves.len(),
                has_expected,
                board_match_count,
                first_match
            );
            eprintln!("best_diff={}, best_move={:?}", best_diff, best_move);
            eprintln!(
                "rot_counts N/E/S/W = {}/{}/{}/{}",
                rot_counts[Rotation::North as usize],
                rot_counts[Rotation::East as usize],
                rot_counts[Rotation::South as usize],
                rot_counts[Rotation::West as usize]
            );
            eprintln!(
                "rot_best_diff N/E/S/W = {}/{}/{}/{}",
                rot_best[Rotation::North as usize],
                rot_best[Rotation::East as usize],
                rot_best[Rotation::South as usize],
                rot_best[Rotation::West as usize]
            );
            eprintln!("expected_move first_row_mismatch={:?}", expected_mismatch);
            eprintln!("floating_count={}", floating_count);
            if reverse_x && !reverse_y {
                for (idx, (diff, rot, x, y, spin)) in top.iter().take(8).enumerate() {
                    eprintln!(
                        "top{}: diff={} rot={:?} x={} y={} spin={:?}",
                        idx, diff, rot, x, y, spin
                    );
                }
                for (diff, rot, x, y, spin) in top.iter() {
                    if *rot == Rotation::East && *diff <= 10 {
                        eprintln!("east-near: diff={} x={} y={} spin={:?}", diff, x, y, spin);
                    }
                }
                eprintln!("expected_added={:?}", expected_added);
                eprintln!(
                    "best_added_overlap={}, best_added_move={:?}",
                    best_added_overlap, best_added_move
                );
                if let Some((rot, x, y, _spin)) = best_move {
                    dump_i_kick_attempts(&board, rot, x, y, rot.ccw(), "best_diff_ccw");
                    dump_i_kick_attempts(&board, rot, x, y, rot.cw(), "best_diff_cw");
                }
                if let Some((rot, x, y, _spin)) = best_added_move {
                    dump_i_kick_attempts(&board, rot, x, y, rot.ccw(), "best_added_ccw");
                    dump_i_kick_attempts(&board, rot, x, y, rot.cw(), "best_added_cw");

                    if rot == Rotation::East {
                        eprintln!(
                            "best_added exhaustive offset scan East->North from ({}, {})",
                            x, y
                        );
                        let to = Rotation::North;
                        let expected_cells = [(6i8, 5i8), (6, 6), (6, 7), (6, 8)];
                        for ox in -3..=3 {
                            for oy in -3..=3 {
                                let tx = x + ox;
                                let mut ty = y + oy;
                                if !can_place(&board, Piece::I, to, tx, ty) {
                                    continue;
                                }
                                while ty > 0 && can_place(&board, Piece::I, to, tx, ty - 1) {
                                    ty -= 1;
                                }

                                let mut cells: Vec<(i8, i8)> = Piece::I
                                    .minos(to)
                                    .iter()
                                    .map(|&(dx, dy)| (tx + dx, ty + dy))
                                    .collect();
                                cells.sort_unstable();
                                let matches_expected = cells == expected_cells;
                                eprintln!(
                                    "  off=({}, {}) target=({}, {}) harddrop_cells={:?} match_expected={}",
                                    ox, oy, tx, ty, cells, matches_expected
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_lock242_r1_strict_frame_ccw_vs_cw_debug() {
        let before_rows = [
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1110111111",
            "1110111111",
            "1110111111",
            "0110000011",
            "0011000011",
            "0000000001",
            "0000000001",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];

        let board = board_from_fixture_rows(&before_rows, true, false);

        let from_rot = Rotation::North;
        let from_x = 4;
        let from_y = 8;

        let ccw = first_legal_i_kick(&board, from_rot, from_x, from_y, from_rot.ccw());
        let cw = first_legal_i_kick(&board, from_rot, from_x, from_y, from_rot.cw());

        eprintln!(
            "strict-frame lock242 from N@({}, {}) ccw={:?} cw={:?}",
            from_x, from_y, ccw, cw
        );
        dump_i_kick_attempts(
            &board,
            from_rot,
            from_x,
            from_y,
            from_rot.ccw(),
            "lock242_ccw",
        );
        dump_i_kick_attempts(
            &board,
            from_rot,
            from_x,
            from_y,
            from_rot.cw(),
            "lock242_cw",
        );

        let moves = generate_moves_bitboard(&board, Piece::I);
        eprintln!("lock242 generated I candidates={}", moves.len());
        for m in moves.iter().filter(|m| m.rotation == Rotation::North) {
            let ccw_m = first_legal_i_kick(&board, m.rotation, m.x, m.y, m.rotation.ccw());
            let cw_m = first_legal_i_kick(&board, m.rotation, m.x, m.y, m.rotation.cw());
            eprintln!(
                "candidate N@({}, {}) ccw={:?} cw={:?}",
                m.x, m.y, ccw_m, cw_m
            );
        }

        let expected = vec![(6, 5), (6, 6), (6, 7), (6, 8)];
        let from_rots = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ];

        eprintln!("lock242 exhaustive all-rotation scan (all legal y)");
        for from_rot in from_rots {
            for x in -2..=11 {
                for y in 0..40 {
                    let y = y as i8;
                    if !can_place(&board, Piece::I, from_rot, x, y) {
                        continue;
                    }

                    let to_ccw = from_rot.ccw();
                    let to_cw = from_rot.cw();
                    let ccw_m = first_legal_i_kick(&board, from_rot, x, y, to_ccw);
                    let cw_m = first_legal_i_kick(&board, from_rot, x, y, to_cw);

                    if let Some((_k, tx, mut ty, _can_fall)) = ccw_m {
                        while ty > 0 && can_place(&board, Piece::I, to_ccw, tx, ty - 1) {
                            ty -= 1;
                        }

                        let mut ccw_cells: Vec<(i8, i8)> = Piece::I
                            .minos(to_ccw)
                            .iter()
                            .map(|&(dx, dy)| (tx + dx, ty + dy))
                            .collect();
                        ccw_cells.sort_unstable();
                        let ccw_matches_expected = ccw_cells == expected;
                        let cw_legal = cw_m.is_some();

                        if ccw_matches_expected || !cw_legal {
                            eprintln!(
                                "{:?}@({}, {}) ccw={:?} -> harddrop_{:?}={:?} cw={:?} match_expected={} cw_legal={}",
                                from_rot,
                                x,
                                y,
                                ccw_m,
                                to_ccw,
                                ccw_cells,
                                cw_m,
                                ccw_matches_expected,
                                cw_legal
                            );
                        }
                    }
                }
            }
        }
    }

    fn cells_for_i_move(m: &Move) -> Vec<(i8, i8)> {
        let mut cells: Vec<(i8, i8)> = Piece::I
            .minos(m.rotation)
            .iter()
            .map(|&(dx, dy)| (m.x + dx, m.y + dy))
            .collect();
        cells.sort_unstable();
        cells
    }

    fn sort_cells<const N: usize>(cells: [(i8, i8); N]) -> Vec<(i8, i8)> {
        let mut out = cells.to_vec();
        out.sort_unstable();
        out
    }

    fn transform_expected_cells(
        expected_cells: &[(i8, i8)],
        reverse_x: bool,
        reverse_y: bool,
    ) -> Vec<(i8, i8)> {
        let mut out: Vec<(i8, i8)> = expected_cells
            .iter()
            .map(|&(x, y)| {
                let tx = if reverse_x { x } else { 9 - x };
                let ty = if reverse_y { 39 - y } else { y };
                (tx, ty)
            })
            .collect();
        out.sort_unstable();
        out
    }

    fn run_i_case_debug(case_name: &str, before_rows: &[&str; 40], expected_cells: [(i8, i8); 4]) {
        let raw_expected = sort_cells(expected_cells);
        let mut any_exact = false;

        for (reverse_x, reverse_y) in [(true, false), (false, false), (true, true), (false, true)] {
            let board = board_from_fixture_rows(before_rows, reverse_x, reverse_y);
            let moves = generate_moves_bitboard(&board, Piece::I);
            let expected = transform_expected_cells(&raw_expected, reverse_x, reverse_y);

            let mut has_exact = false;
            let mut best_overlap = 0usize;
            let mut best_overlap_move: Option<(Rotation, i8, i8, SpinType, Vec<(i8, i8)>)> = None;

            for m in &moves {
                let cells = cells_for_i_move(m);
                if cells == expected {
                    has_exact = true;
                }
                let overlap = cells.iter().filter(|cell| expected.contains(cell)).count();
                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_overlap_move = Some((m.rotation, m.x, m.y, m.spin_type, cells));
                }
            }

            eprintln!(
                "{} rx={} ry={}: candidates={} has_exact={} expected={:?} best_overlap={} best={:?}",
                case_name,
                reverse_x,
                reverse_y,
                moves.len(),
                has_exact,
                expected,
                best_overlap,
                best_overlap_move
            );

            any_exact |= has_exact;
        }

        assert!(any_exact, "{} missing expected I placement", case_name);
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_1_replay_36_vs_akiairi() {
        let before_rows = [
            "1101111111",
            "1101111111",
            "1101111111",
            "1101111111",
            "1101111111",
            "1011111111",
            "1011111111",
            "1111011111",
            "1111011111",
            "1111011111",
            "1111011111",
            "1111011111",
            "1111011111",
            "1011111111",
            "1011111111",
            "1011111111",
            "1000000011",
            "1100000111",
            "0000000011",
            "0000000010",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case1_36_vs_akiairi_r0_f1131_l42",
            &before_rows,
            [(8, 13), (8, 14), (8, 15), (8, 16)],
        );
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_2_replay_36_vs_azelis() {
        let before_rows = [
            "1111111101",
            "1111111101",
            "1111101111",
            "1110111111",
            "1110111111",
            "1110111111",
            "1110000011",
            "1111000011",
            "1101000001",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case2_36_vs_azelis_r0_f2135_l111",
            &before_rows,
            [(6, 3), (6, 4), (6, 5), (6, 6)],
        );
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_3_replay_36_vs_master101() {
        let before_rows = [
            "1111111110",
            "1111111110",
            "1111111110",
            "1111111110",
            "1111111110",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111100111",
            "1110111111",
            "1110111111",
            "1110111111",
            "1110000011",
            "1111100011",
            "1111000011",
            "1000000011",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case3_36_vs_master101_r3_f1282_l597",
            &before_rows,
            [(6, 12), (6, 13), (6, 14), (6, 15)],
        );
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_4_replay_36_vs_sserza() {
        let before_rows = [
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1110111111",
            "1111111110",
            "1110111111",
            "1110111111",
            "1110111111",
            "0110000010",
            "0111000000",
            "0111000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case4_36_vs_sserza_r2_f1051_l789",
            &before_rows,
            [(6, 8), (6, 9), (6, 10), (6, 11)],
        );
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_5_replay_36_vs_thorns() {
        let before_rows = [
            "1111011111",
            "1111011111",
            "1111011111",
            "1111011111",
            "1111011111",
            "1110111111",
            "1110111111",
            "1110111111",
            "1110000111",
            "1011000111",
            "0000000111",
            "0000000001",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case5_36_vs_thorns_r7_f1219_l794",
            &before_rows,
            [(6, 5), (6, 6), (6, 7), (6, 8)],
        );
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_6_replay_caboozled_vs_naraa() {
        let before_rows = [
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1110111111",
            "1110111111",
            "1110111111",
            "0110000011",
            "0011000011",
            "0000000001",
            "0000000001",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case6_caboozled_vs_naraa_r1_f2802_l242",
            &before_rows,
            [(6, 5), (6, 6), (6, 7), (6, 8)],
        );
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_7_replay_firestorm_vs_supersonic() {
        let before_rows = [
            "1110111111",
            "1110111111",
            "1110111111",
            "1110111111",
            "1110111111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1111101111",
            "1011111111",
            "1011111111",
            "1011111111",
            "1000000101",
            "1100000100",
            "0100000100",
            "0000001100",
            "0000001000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case7_firestorm_vs_supersonic_r8_f755_l806",
            &before_rows,
            [(8, 11), (8, 12), (8, 13), (8, 14)],
        );
    }

    #[test]
    #[ignore = "debug fixture parity"]
    fn test_i_case_8_replay_tiki2tgt_vs_apollo18() {
        let before_rows = [
            "1011111111",
            "1011111111",
            "1011111111",
            "1011111111",
            "1011111111",
            "1110111111",
            "1111011111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1111110111",
            "1101111111",
            "1101111111",
            "1101111111",
            "1100000011",
            "0110000001",
            "0010000001",
            "0000000001",
            "0000000001",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
            "0000000000",
        ];
        run_i_case_debug(
            "case8_tiki2tgt_vs_apollo18_r4_f1755_l536",
            &before_rows,
            [(7, 15), (7, 16), (7, 17), (7, 18)],
        );
    }
}
