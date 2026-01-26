use crate::misdrop::{Misdrop, MisdropSeverity};
use fusion_core::{Board, Move, Piece, SpinType};

#[derive(Debug, Clone)]
pub struct Moment {
    pub frame: u32,
    pub moment_type: MomentType,
    pub description: String,
    pub suggestion: Option<String>,
    pub impact: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MomentType {
    Misdrop(MisdropSeverity),
    MissedTSpin,
    InefficientClear,
    GoodPlay,
    ClutchSave,
}

pub fn generate_moments(misdrops: &[Misdrop], _stats: &GameStats) -> Vec<Moment> {
    let mut moments = Vec::new();

    for md in misdrops {
        moments.push(Moment {
            frame: md.frame,
            moment_type: MomentType::Misdrop(md.severity),
            description: format!(
                "Placed {:?} at ({}, {}) instead of ({}, {})",
                md.player_move.piece,
                md.player_move.x,
                md.player_move.y,
                md.best_move.x,
                md.best_move.y
            ),
            suggestion: Some(format!(
                "Try {:?} rotation at column {}",
                md.best_move.rotation, md.best_move.x
            )),
            impact: md.score_diff,
        });
    }

    moments.sort_by_key(|m| m.frame);
    moments
}

#[derive(Debug, Default)]
pub struct GameStats {
    pub total_pieces: u32,
    pub misdrops: u32,
    pub lines_cleared: u32,
    pub attack_sent: u32,
    pub max_combo: u32,
    pub max_b2b: u32,
    pub tspins: u32,
    pub quads: u32,
}

pub fn detect_missed_tspin(
    board: &Board,
    piece: Piece,
    player_move: &Move,
    frame: u32,
) -> Option<Moment> {
    if piece != Piece::T {
        return None;
    }

    if player_move.spin_type != SpinType::None {
        return None;
    }

    // Move.x/y is the rotation center for T in this codebase (matches movement.rs)
    let center_x = player_move.x;
    let center_y = player_move.y;

    let corners = [
        (center_x - 1, center_y - 1), // Bottom-left relative to center
        (center_x + 1, center_y - 1), // Bottom-right relative to center
        (center_x - 1, center_y + 1), // Top-left relative to center
        (center_x + 1, center_y + 1), // Top-right relative to center
    ];

    let occupied_corners = corners
        .iter()
        .filter(|&&(x, y)| {
            // Walls and floor count as occupied
            if x < 0 || x >= Board::WIDTH as i8 || y < 0 {
                return true;
            }
            // Ceiling (y >= HEIGHT) is empty
            if y >= Board::HEIGHT as i8 {
                return false;
            }
            // Check board cell
            board.get(x as usize, y as usize)
        })
        .count();

    if occupied_corners >= 3 {
        Some(Moment {
            frame,
            moment_type: MomentType::MissedTSpin,
            description: "Missed T-Spin opportunity".to_string(),
            suggestion: Some("Look for T-Spin setups".to_string()),
            impact: 0.0,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::{Move, Piece, Rotation};

    fn sample_misdrop(frame: u32, severity: MisdropSeverity) -> Misdrop {
        Misdrop {
            frame,
            player_move: Move::new(Piece::T, Rotation::North, 4, 0),
            best_move: Move::new(Piece::I, Rotation::East, 5, 0),
            player_score: -10.0,
            best_score: 20.0,
            score_diff: 30.0,
            creates_hole: false,
            severity,
        }
    }

    #[test]
    fn test_generate_moment_from_misdrop() {
        let stats = GameStats::default();
        let misdrop = sample_misdrop(8, MisdropSeverity::Moderate);

        let moments = generate_moments(&[misdrop], &stats);
        assert_eq!(moments.len(), 1);
        assert_eq!(moments[0].frame, 8);
        assert_eq!(
            moments[0].moment_type,
            MomentType::Misdrop(MisdropSeverity::Moderate)
        );
        assert!(moments[0].suggestion.is_some());
    }

    #[test]
    fn test_moments_sorted_by_frame() {
        let stats = GameStats::default();
        let first = sample_misdrop(20, MisdropSeverity::Minor);
        let second = sample_misdrop(5, MisdropSeverity::Major);

        let moments = generate_moments(&[first, second], &stats);
        assert_eq!(moments[0].frame, 5);
        assert_eq!(moments[1].frame, 20);
    }

    #[test]
    fn test_detect_missed_tspin_none_for_non_t_piece() {
        let board = Board::new();
        let piece = Piece::I;
        let player_move = Move::new(piece, Rotation::North, 4, 0);
        let result = detect_missed_tspin(&board, piece, &player_move, 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_missed_tspin_none_when_tspin_used() {
        let board = Board::new();
        let piece = Piece::T;
        let mut player_move = Move::new(piece, Rotation::North, 4, 0);
        player_move.spin_type = SpinType::Full; // Assuming Full exists, or we check compilation
        let result = detect_missed_tspin(&board, piece, &player_move, 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_missed_tspin_detects_opportunity() {
        let mut board = Board::new();
        // Setup a T-spin slot.
        // T center at (4, 1).
        // Corners: (3, 0), (5, 0), (3, 2), (5, 2).
        // Occupy 3 corners.
        // (3,0) -> Left-Bottom.
        // (5,0) -> Right-Bottom.
        // (3,2) -> Left-Top.

        // Note: Move::new takes (piece, rotation, x, y) where x/y is center.

        board.set(3, 0, true);
        board.set(5, 0, true);
        board.set(3, 2, true);
        // (5, 2) is empty.

        let piece = Piece::T;
        let player_move = Move::new(piece, Rotation::North, 4, 1);
        // spin_type is None by default

        let result = detect_missed_tspin(&board, piece, &player_move, 100);
        assert!(result.is_some());
        let moment = result.unwrap();
        assert_eq!(moment.moment_type, MomentType::MissedTSpin);
    }
}
