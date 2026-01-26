use fusion_core::{Board, Move, Piece};
use fusion_eval::{count_holes, evaluate_with_clear, EvalWeights};
use fusion_search::{apply_move, BeamSearch};

#[derive(Debug, Clone)]
pub struct Misdrop {
    pub frame: u32,
    pub player_move: Move,
    pub best_move: Move,
    pub player_score: f32,
    pub best_score: f32,
    pub score_diff: f32,
    pub creates_hole: bool,
    pub severity: MisdropSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MisdropSeverity {
    Minor,
    Moderate,
    Major,
}

pub fn detect_misdrop(
    board: &Board,
    piece: Piece,
    player_move: &Move,
    frame: u32,
) -> Option<Misdrop> {
    if player_move.piece != piece {
        return None;
    }
    let search = BeamSearch::default();
    let (best_move, _) = search.find_best_move(board, piece)?;

    let (player_board, player_lines) = apply_move(board, player_move);
    let (best_board, best_lines) = apply_move(board, &best_move);

    let weights = EvalWeights::default();
    let player_score = evaluate_with_clear(&player_board, player_lines, &weights);
    let best_score = evaluate_with_clear(&best_board, best_lines, &weights);
    let score_diff = best_score - player_score;

    if score_diff > 20.0 {
        Some(Misdrop {
            frame,
            player_move: *player_move,
            best_move,
            player_score,
            best_score,
            score_diff,
            creates_hole: count_new_holes(board, &player_board) > 0,
            severity: classify_severity(score_diff),
        })
    } else {
        None
    }
}

fn classify_severity(diff: f32) -> MisdropSeverity {
    if diff.is_nan() || diff < 50.0 {
        MisdropSeverity::Minor
    } else if diff < 150.0 {
        MisdropSeverity::Moderate
    } else {
        MisdropSeverity::Major
    }
}

fn count_new_holes(before: &Board, after: &Board) -> u32 {
    count_holes(after).saturating_sub(count_holes(before))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::Rotation;

    #[test]
    fn test_misdrop_detects_obvious_bad_move() {
        let board = Board::new();
        let piece = Piece::T;
        let player_move = Move::new(piece, Rotation::North, 4, 10);

        let result = detect_misdrop(&board, piece, &player_move, 12);
        assert!(result.is_some());

        let misdrop = result.expect("expected misdrop");
        assert!(misdrop.score_diff > 20.0);
        assert!(misdrop.creates_hole);
    }

    #[test]
    fn test_good_move_not_flagged() {
        let board = Board::new();
        let piece = Piece::T;
        let search = BeamSearch::default();
        let (best_move, _) = search
            .find_best_move(&board, piece)
            .expect("expected best move");

        let result = detect_misdrop(&board, piece, &best_move, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_classify_severity_thresholds() {
        assert_eq!(classify_severity(10.0), MisdropSeverity::Minor);
        assert_eq!(classify_severity(50.0), MisdropSeverity::Moderate);
        assert_eq!(classify_severity(149.9), MisdropSeverity::Moderate);
        assert_eq!(classify_severity(150.0), MisdropSeverity::Major);
        assert_eq!(classify_severity(150.1), MisdropSeverity::Major);
    }
}
