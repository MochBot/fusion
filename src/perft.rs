// perft.rs -- perft with bulk counting, clone+do_move
use crate::board::Board;
use crate::header::*;
use crate::movegen::MoveList;
#[cfg(feature = "rayon")]
use rayon::prelude::*;

const QUEUE: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
    Piece::T,
];

fn queue_piece(depth: usize) -> Piece {
    QUEUE[depth % 7]
}

/// serial perft — clone + do_move, bulk counting at depth 1
pub fn perft(board: &Board, queue_offset: usize, depth: usize) -> u64 {
    if depth == 0 {
        return 1;
    }

    let piece = queue_piece(queue_offset);
    let ml = MoveList::new(board, piece);

    if ml.is_empty() {
        return 0;
    }

    // bulk counting: at depth 1, just return move count
    if depth == 1 {
        return ml.size() as u64;
    }

    let mut nodes: u64 = 0;
    for m in ml.iter() {
        let mut child = board.clone();
        child.do_move(m);
        nodes += perft(&child, queue_offset + 1, depth - 1);
    }
    nodes
}

/// divide: print per-move breakdown at root
pub fn divide(board: &Board, depth: usize) -> u64 {
    let piece = queue_piece(0);
    let ml = MoveList::new(board, piece);
    let mut total: u64 = 0;

    for m in ml.iter() {
        let mut child = board.clone();
        child.do_move(m);
        let count = perft(&child, 1, depth - 1);
        println!(
            "{:?} ({},{}) r={:?}: {}",
            m.piece(),
            m.x(),
            m.y(),
            m.rotation(),
            count
        );
        total += count;
    }
    println!("Total: {total}");
    total
}

/// parallel perft — two-level work split for high core saturation
pub fn perft_parallel(board: &Board, depth: usize) -> u64 {
    if depth <= 2 {
        return perft(board, 0, depth);
    }

    // expand first 2 plies into work units
    let piece0 = queue_piece(0);
    let ml0 = MoveList::new(board, piece0);

    let work_units: Vec<Board> = ml0
        .iter()
        .flat_map(|m0| {
            let mut b1 = board.clone();
            b1.do_move(m0);
            let piece1 = queue_piece(1);
            let ml1 = MoveList::new(&b1, piece1);
            ml1.iter()
                .map(|m1| {
                    let mut b2 = b1.clone();
                    b2.do_move(m1);
                    b2
                })
                .collect::<Vec<_>>()
        })
        .collect();

    #[cfg(feature = "rayon")]
    {
        work_units.par_iter().map(|b| perft(b, 2, depth - 2)).sum()
    }
    #[cfg(not(feature = "rayon"))]
    {
        work_units.iter().map(|b| perft(b, 2, depth - 2)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D1: u64 = 17;
    const D2: u64 = 153;
    const D3: u64 = 5266;
    const D4: u64 = 188561;
    const D5: u64 = 3573524;

    #[test]
    fn test_perft_d1() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 1), D1);
    }

    #[test]
    fn test_perft_d2() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 2), D2);
    }

    #[test]
    fn test_perft_d3() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 3), D3);
    }

    #[test]
    fn test_perft_d4() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 4), D4);
    }

    #[test]
    fn test_perft_d5() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 5), D5);
    }
}
