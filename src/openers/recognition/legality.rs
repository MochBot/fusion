use crate::board::{Board, LockMechanics, BOARD_HEIGHT, FULL_ROW};
use crate::gen::{canonical_r, CollisionMap};
use crate::header::{bb, piece_table, Bitboard, Move, Piece, Rotation, COL_NB};
use crate::move_buffer::MoveBuffer;
use crate::pathfinder::{reachable_locks, ReachLocks};
use crate::smear_core::generate_placements;
use smallvec::SmallVec;

#[derive(Clone)]
pub(crate) enum LegalityVerdict {
    Legal {
        target: Move,
        mechanics: LockMechanics,
    },
    UnreachableFromSpawn,
    Unsupported,
    NotAPlacement,
}

#[derive(Clone)]
pub(crate) struct LegalityCheck {
    pub(crate) support_valid: bool,
    #[cfg(test)]
    pub(crate) geometry_emitted: bool,
    #[cfg(test)]
    pub(crate) label_free_reachable: bool,
    #[cfg(test)]
    pub(crate) spin_labelled_reachable: bool,
    pub(crate) verdict: LegalityVerdict,
}

pub(crate) fn placement_is_srs_legal(
    board: &Board,
    letter: u8,
    cells: &[[u8; 2]; 4],
) -> LegalityCheck {
    let Some(piece) = piece_for_letter(letter) else {
        return LegalityCheck {
            support_valid: false,
            #[cfg(test)]
            geometry_emitted: false,
            #[cfg(test)]
            label_free_reachable: false,
            #[cfg(test)]
            spin_labelled_reachable: false,
            verdict: LegalityVerdict::NotAPlacement,
        };
    };
    let expected = sorted_cells(*cells);
    if let Some(bare) = dropped_placement(board, piece, &expected) {
        let check = legal_check(bare, lock_mechanics(board, bare), true, true, false);
        debug_assert!(same_check(&check, &searched_placement(board, piece, expected)));
        return check;
    }
    searched_placement(board, piece, expected)
}

/// The canonical placement covering `expected` when it rests on the stack and
/// drops straight in from open air: movegen emits exactly one move per distinct
/// placement, so it would be the only candidate, found reachable as it is.
fn dropped_placement(board: &Board, piece: Piece, expected: &[[u8; 2]; 4]) -> Option<Move> {
    if board.rows[OPEN_AIR_ROW..].iter().any(|&row| row != 0) {
        return None;
    }
    let bare = canonical_placement(piece, expected)?;
    if !board.legal_lock_placement(&bare) {
        return None;
    }
    drops_from_open_air(board, &board.compute_cols(), bare).then_some(bare)
}

/// The canonical-rotation move whose cells are `expected`. Per rotation the
/// shape's least cell (in `sorted_cells` order) lands on `expected[0]`, which
/// places the pivot.
fn canonical_placement(piece: Piece, expected: &[[u8; 2]; 4]) -> Option<Move> {
    [Rotation::North, Rotation::East, Rotation::South, Rotation::West]
        .into_iter()
        .filter(|rotation| canonical_r(piece, *rotation) == *rotation)
        .filter_map(|rotation| {
            let least = piece_table(piece, rotation)
                .coords
                .iter()
                .map(|offset| (i32::from(offset.x), i32::from(offset.y)))
                .fold((0, 0), std::cmp::min);
            let [x, y] = expected[0];
            Some(Move::new(
                piece,
                rotation,
                i32::from(x) - least.0,
                i32::from(y) - least.1,
                false,
            ))
            .filter(|candidate| placement_cells(*candidate) == Some(*expected))
        })
        .next()
}

fn same_check(left: &LegalityCheck, right: &LegalityCheck) -> bool {
    let verdicts = match (&left.verdict, &right.verdict) {
        (
            LegalityVerdict::Legal { target, mechanics },
            LegalityVerdict::Legal {
                target: other_target,
                mechanics: other_mechanics,
            },
        ) => target == other_target && mechanics == other_mechanics,
        (LegalityVerdict::UnreachableFromSpawn, LegalityVerdict::UnreachableFromSpawn)
        | (LegalityVerdict::Unsupported, LegalityVerdict::Unsupported)
        | (LegalityVerdict::NotAPlacement, LegalityVerdict::NotAPlacement) => true,
        _ => false,
    };
    #[cfg(test)]
    let verdicts = verdicts
        && left.geometry_emitted == right.geometry_emitted
        && left.label_free_reachable == right.label_free_reachable
        && left.spin_labelled_reachable == right.spin_labelled_reachable;
    verdicts && left.support_valid == right.support_valid
}

/// The full check: every reachable placement of `piece`, then the one on `expected`.
fn searched_placement(board: &Board, piece: Piece, expected: [[u8; 2]; 4]) -> LegalityCheck {
    let mut moves = MoveBuffer::new();
    generate_placements(board, &mut moves, piece, false);
    let candidates = moves
        .iter()
        .copied()
        .filter(|candidate| {
            holds_pivot(&expected, *candidate) && placement_cells(*candidate) == Some(expected)
        })
        .collect::<SmallVec<[Move; 4]>>();
    if candidates.is_empty() {
        return LegalityCheck {
            support_valid: false,
            #[cfg(test)]
            geometry_emitted: false,
            #[cfg(test)]
            label_free_reachable: false,
            #[cfg(test)]
            spin_labelled_reachable: false,
            verdict: LegalityVerdict::NotAPlacement,
        };
    }
    let geometry_emitted = true;
    let mut support_valid = false;
    let mut label_free_reachable = false;
    let mut spin_labelled_reachable = false;
    let mut cols = None;
    let mut reach = None;
    // Labeled movegen emits exactly the pathfinder's reachable set; the
    // pathfinder's seeded parity tests pin that, not a search per call here.
    let mut reachable = |target: Move| {
        lock_emitted(
            reach.get_or_insert_with(|| labeled_locks(board, piece)),
            target,
        )
    };

    for candidate in candidates {
        if !board.legal_lock_placement(&candidate) {
            continue;
        }
        support_valid = true;
        let bare = Move::new(
            piece,
            candidate.rotation(),
            candidate.x(),
            candidate.y(),
            false,
        );
        let cols = cols.get_or_insert_with(|| board.compute_cols());
        let dropped = drops_from_open_air(board, cols, bare);
        debug_assert!(!dropped || lock_reachable(&reachable_locks(board, piece, false), bare));
        if dropped || reachable(bare) {
            label_free_reachable = true;
            let mechanics = lock_mechanics(board, bare);
            return legal_check(
                bare,
                mechanics,
                geometry_emitted,
                label_free_reachable,
                spin_labelled_reachable,
            );
        }
        for spin_target in spin_targets(piece, bare) {
            if !reachable(spin_target) {
                continue;
            }
            spin_labelled_reachable = true;
            let mechanics = lock_mechanics(board, spin_target);
            return legal_check(
                spin_target,
                mechanics,
                geometry_emitted,
                label_free_reachable,
                spin_labelled_reachable,
            );
        }
    }

    LegalityCheck {
        support_valid,
        #[cfg(test)]
        geometry_emitted,
        #[cfg(test)]
        label_free_reachable,
        #[cfg(test)]
        spin_labelled_reachable,
        verdict: if support_valid {
            LegalityVerdict::UnreachableFromSpawn
        } else {
            LegalityVerdict::Unsupported
        },
    }
}

pub(crate) fn engine_board_from_masks(masks: &[u16]) -> Board {
    let mut rows = [0u16; BOARD_HEIGHT];
    for (y, mask) in masks.iter().copied().enumerate().take(BOARD_HEIGHT) {
        rows[y] = mask & FULL_ROW;
    }
    Board::from_rows(rows)
}

pub(crate) fn placement_cells(target: Move) -> Option<[[u8; 2]; 4]> {
    let offsets = target.cells();
    let coordinates = [
        (target.x(), target.y()),
        (
            target.x() + i32::from(offsets[0].x),
            target.y() + i32::from(offsets[0].y),
        ),
        (
            target.x() + i32::from(offsets[1].x),
            target.y() + i32::from(offsets[1].y),
        ),
        (
            target.x() + i32::from(offsets[2].x),
            target.y() + i32::from(offsets[2].y),
        ),
    ];
    let mut cells = [[0; 2]; 4];
    for (index, (x, y)) in coordinates.into_iter().enumerate() {
        cells[index] = [u8::try_from(x).ok()?, u8::try_from(y).ok()?];
    }
    Some(sorted_cells(cells))
}

/// A placement's pivot is one of its cells, so a candidate whose pivot is not
/// among `expected` cannot match: a cheap reject before sorting its cells.
fn holds_pivot(expected: &[[u8; 2]; 4], candidate: Move) -> bool {
    match (u8::try_from(candidate.x()), u8::try_from(candidate.y())) {
        (Ok(x), Ok(y)) => expected.contains(&[x, y]),
        _ => false,
    }
}

fn piece_for_letter(letter: u8) -> Option<Piece> {
    match letter {
        b'I' => Some(Piece::I),
        b'O' => Some(Piece::O),
        b'T' => Some(Piece::T),
        b'L' => Some(Piece::L),
        b'J' => Some(Piece::J),
        b'S' => Some(Piece::S),
        b'Z' => Some(Piece::Z),
        _ => None,
    }
}

fn sorted_cells(mut cells: [[u8; 2]; 4]) -> [[u8; 2]; 4] {
    cells.sort_unstable();
    cells
}

/// Memoizes the exact legality verdict for a physical board, piece letter,
/// and sorted cells within one compilation. The verdict is a pure function
/// of those inputs, so repeated queries across DFS parents and branches pay
/// movegen plus pathfinding once.
#[derive(Default)]
pub(crate) struct LegalityMemo {
    // Fx, not SipHash: the 89-byte key is hashed on every check; never iterated.
    entries: std::collections::HashMap<
        LegalityKey,
        LegalityCheck,
        std::hash::BuildHasherDefault<crate::coach_beam::FxHasher64>,
    >,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct LegalityKey {
    rows: [u16; BOARD_HEIGHT],
    letter: u8,
    cells: [[u8; 2]; 4],
}

impl LegalityMemo {
    pub(crate) fn check(
        &mut self,
        board: &Board,
        letter: u8,
        cells: &[[u8; 2]; 4],
    ) -> LegalityCheck {
        let key = LegalityKey {
            rows: board.rows,
            letter,
            cells: sorted_cells(*cells),
        };
        if let Some(hit) = self.entries.get(&key) {
            return hit.clone();
        }
        let verdict = placement_is_srs_legal(board, letter, cells);
        self.entries.insert(key, verdict.clone());
        verdict
    }
}

/// The lowest row the spawn search treats as open air: with every row from here
/// up empty, the spawn rotations (kick 0 at rows 20-22) and shifts reach every
/// in-bounds column and rotation at spawn height.
const OPEN_AIR_ROW: usize = 14;

/// Whether `bare` rests where its column is clear up to open air, so the spawn
/// search drops into it with no spin label. The clear cell above also rules out
/// an immobile arrival, the only way a lock there could be relabelled Mini.
fn drops_from_open_air(board: &Board, cols: &[Bitboard; COL_NB], bare: Move) -> bool {
    let column = CollisionMap::column(cols, bare.piece(), bare.rotation(), bare.x());
    let y = bare.y();
    board.rows[OPEN_AIR_ROW..].iter().all(|&row| row == 0)
        && column >> y == 0
        && (y == 0 || column & bb(y - 1) != 0)
}

/// Whether the pathfinder finds a key path that locks exactly `target`, label
/// included: one spawn search answers every candidate a check asks about, where
/// `get_input` would search again per target and exhaust the space on a miss.
fn lock_reachable(reach: &ReachLocks, target: Move) -> bool {
    // Movegen emits canonical rotations, the frame `reachable_locks` indexes by.
    debug_assert_eq!(
        target.rotation(),
        canonical_r(reach.piece, target.rotation())
    );
    let rotation = target.rotation() as usize;
    reach.locks[target.spin() as usize][target.x() as usize][rotation] & bb(target.y()) != 0
}

/// Every lock the spawn search reaches, label included: labeled movegen emits
/// exactly `reachable_locks`'s set (`assert_pathfinder_matches_strict` pins it)
/// for a fraction of the pathfinder's breadth-first search.
fn labeled_locks(board: &Board, piece: Piece) -> MoveBuffer {
    let mut moves = MoveBuffer::new();
    crate::movegen::generate(board, &mut moves, piece, false);
    moves
}

/// `lock_reachable` over the labeled movegen set.
fn lock_emitted(labeled: &MoveBuffer, target: Move) -> bool {
    labeled.iter().any(|m| {
        m.spin() as usize == target.spin() as usize
            && m.x() == target.x()
            && m.y() == target.y()
            && canonical_r(target.piece(), m.rotation()) == target.rotation()
    })
}

fn spin_targets(piece: Piece, bare: Move) -> SmallVec<[Move; 2]> {
    match piece {
        Piece::T => smallvec::smallvec![
            Move::new_tspin(bare.rotation(), bare.x(), bare.y(), false),
            Move::new_tspin(bare.rotation(), bare.x(), bare.y(), true),
        ],
        Piece::O => SmallVec::new(),
        _ => smallvec::smallvec![Move::new_allspin_mini(
            piece,
            bare.rotation(),
            bare.x(),
            bare.y(),
        )],
    }
}

fn lock_mechanics(board: &Board, target: Move) -> LockMechanics {
    let mut locked = board.clone();
    locked.lock(&target)
}

fn legal_check(
    target: Move,
    mechanics: LockMechanics,
    _geometry_emitted: bool,
    _label_free_reachable: bool,
    _spin_labelled_reachable: bool,
) -> LegalityCheck {
    LegalityCheck {
        support_valid: true,
        #[cfg(test)]
        geometry_emitted: _geometry_emitted,
        #[cfg(test)]
        label_free_reachable: _label_free_reachable,
        #[cfg(test)]
        spin_labelled_reachable: _spin_labelled_reachable,
        verdict: LegalityVerdict::Legal { target, mechanics },
    }
}
