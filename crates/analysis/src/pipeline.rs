use crate::misdrop::{detect_misdrop, Misdrop, MisdropSeverity};
use crate::moments::{generate_moments, GameStats, Moment};
use fusion_core::{Board, Move, Piece};

#[derive(Debug, Clone)]
pub struct ReplayFrame {
    pub frame_number: u32,
    pub piece: Piece,
    pub player_move: Move,
    pub board_before: Board,
    pub lines_cleared: u8,
}

#[derive(Debug)]
pub struct AnalysisResult {
    pub moments: Vec<Moment>,
    pub stats: GameStats,
    pub misdrops: Vec<Misdrop>,
    pub overall_score: f32,
}

pub fn analyze_replay(frames: &[ReplayFrame]) -> AnalysisResult {
    let mut misdrops = Vec::new();
    let mut stats = GameStats::default();

    for frame in frames {
        stats.total_pieces += 1;
        stats.lines_cleared += frame.lines_cleared as u32;

        if let Some(misdrop) = detect_misdrop(
            &frame.board_before,
            frame.piece,
            &frame.player_move,
            frame.frame_number,
        ) {
            misdrops.push(misdrop);
            stats.misdrops += 1;
        }
    }

    let moments = generate_moments(&misdrops, &stats);
    let overall_score = calculate_performance_score(&stats, &misdrops);

    AnalysisResult {
        moments,
        stats,
        misdrops,
        overall_score,
    }
}

fn calculate_performance_score(stats: &GameStats, misdrops: &[Misdrop]) -> f32 {
    if stats.total_pieces == 0 {
        return 100.0;
    }

    let misdrop_rate = stats.misdrops as f32 / stats.total_pieces as f32;
    let base_score = 100.0 * (1.0 - misdrop_rate);

    let severity_penalty: f32 = misdrops
        .iter()
        .map(|misdrop| match misdrop.severity {
            MisdropSeverity::Minor => 0.5,
            MisdropSeverity::Moderate => 1.5,
            MisdropSeverity::Major => 3.0,
        })
        .sum();

    (base_score - severity_penalty).max(0.0).min(100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::Rotation;
    use fusion_search::BeamSearch;

    fn sample_misdrop(severity: MisdropSeverity) -> Misdrop {
        Misdrop {
            frame: 0,
            player_move: Move::new(Piece::T, Rotation::North, 4, 0),
            best_move: Move::new(Piece::T, Rotation::North, 4, 0),
            player_score: 0.0,
            best_score: 0.0,
            score_diff: 0.0,
            creates_hole: false,
            severity,
        }
    }

    #[test]
    fn test_empty_replay_analysis() {
        let result = analyze_replay(&[]);
        assert_eq!(result.stats.total_pieces, 0);
        assert_eq!(result.stats.misdrops, 0);
        assert!(result.moments.is_empty());
        assert!(result.misdrops.is_empty());
        assert_eq!(result.overall_score, 100.0);
    }

    #[test]
    fn test_performance_score_calculation() {
        let stats = GameStats {
            total_pieces: 10,
            misdrops: 2,
            ..GameStats::default()
        };
        let misdrops = vec![
            sample_misdrop(MisdropSeverity::Minor),
            sample_misdrop(MisdropSeverity::Major),
        ];

        let score = calculate_performance_score(&stats, &misdrops);
        assert!((score - 76.5).abs() < 0.01);
    }

    #[test]
    fn test_stats_accumulation() {
        let board = Board::new();
        let bad_move = Move::new(Piece::T, Rotation::North, 4, 10);
        let (best_move, _) = BeamSearch::default()
            .find_best_move(&board, Piece::I)
            .expect("expected best move");

        let frames = vec![
            ReplayFrame {
                frame_number: 1,
                piece: Piece::T,
                player_move: bad_move,
                board_before: board.clone(),
                lines_cleared: 1,
            },
            ReplayFrame {
                frame_number: 2,
                piece: Piece::I,
                player_move: best_move,
                board_before: board,
                lines_cleared: 2,
            },
        ];

        let result = analyze_replay(&frames);
        assert_eq!(result.stats.total_pieces, 2);
        assert_eq!(result.stats.lines_cleared, 3);
        assert_eq!(result.stats.misdrops, 1);
    }
}
