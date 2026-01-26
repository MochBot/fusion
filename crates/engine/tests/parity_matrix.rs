use fusion_core::{Board, Piece, Rotation, SpinType};
use fusion_engine::movement::detect_all_spin;
use fusion_engine::{calculate_attack, AttackConfig, B2BTracker, ChargingConfig};

fn assert_attack(
    lines: u8,
    spin: SpinType,
    b2b: u8,
    combo: u8,
    config: &AttackConfig,
    is_perfect_clear: bool,
    expected: u8,
) {
    let result = calculate_attack(lines, spin, b2b, combo, config, is_perfect_clear);
    assert_eq!(result.floor() as u8, expected);
}

fn caged_board(piece: Piece, rotation: Rotation, x: i8, y: i8) -> Board {
    let mut board = Board::new();
    for row in 0..Board::HEIGHT {
        for col in 0..Board::WIDTH {
            board.set(col, row, true);
        }
    }
    for (dx, dy) in piece.minos(rotation) {
        let nx = (x + dx) as usize;
        let ny = (y + dy) as usize;
        board.set(nx, ny, false);
    }
    board
}

fn clear_cell(board: &mut Board, x: i8, y: i8) {
    if x >= 0 && x < Board::WIDTH as i8 && y >= 0 && y < Board::HEIGHT as i8 {
        board.set(x as usize, y as usize, false);
    }
}

fn surge_after_break(level: u8, charging: ChargingConfig) -> Option<[u8; 3]> {
    let mut tracker = B2BTracker::new(false, Some(charging));
    for _ in 0..level {
        tracker.register_clear(4, SpinType::None);
    }
    tracker.register_clear(1, SpinType::None).surge
}

mod basic_clears {
    use super::*;

    #[test]
    fn test_1_1_single_no_b2b_no_combo() {
        let config = AttackConfig::tetra_league();
        assert_attack(1, SpinType::None, 0, 0, &config, false, 0);
    }

    #[test]
    fn test_1_2_double_no_b2b_no_combo() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::None, 0, 0, &config, false, 1);
    }

    #[test]
    fn test_1_3_triple_no_b2b_no_combo() {
        let config = AttackConfig::tetra_league();
        assert_attack(3, SpinType::None, 0, 0, &config, false, 2);
    }

    #[test]
    fn test_1_4_quad_no_b2b_no_combo() {
        let config = AttackConfig::tetra_league();
        assert_attack(4, SpinType::None, 0, 0, &config, false, 4);
    }
}

mod tspin_clears {
    use super::*;

    #[test]
    fn test_2_1_mini_zero_lines() {
        let config = AttackConfig::tetra_league();
        assert_attack(0, SpinType::Mini, 0, 0, &config, false, 0);
    }

    #[test]
    fn test_2_2_mini_single() {
        let config = AttackConfig::tetra_league();
        assert_attack(1, SpinType::Mini, 0, 0, &config, false, 0);
    }

    #[test]
    fn test_2_3_mini_double() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::Mini, 0, 0, &config, false, 1);
    }

    #[test]
    fn test_2_4_full_single() {
        let config = AttackConfig::tetra_league();
        assert_attack(1, SpinType::Full, 0, 0, &config, false, 2);
    }

    #[test]
    fn test_2_5_full_double() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::Full, 0, 0, &config, false, 4);
    }

    #[test]
    fn test_2_6_full_triple() {
        let config = AttackConfig::tetra_league();
        assert_attack(3, SpinType::Full, 0, 0, &config, false, 6);
    }
}

mod b2b_bonus {
    use super::*;

    #[test]
    fn test_3_1_quad_b2b_1() {
        let config = AttackConfig::tetra_league();
        assert_attack(4, SpinType::None, 1, 0, &config, false, 5);
    }

    #[test]
    fn test_3_2_quad_b2b_5() {
        let config = AttackConfig::tetra_league();
        assert_attack(4, SpinType::None, 5, 0, &config, false, 5);
    }

    #[test]
    fn test_3_3_tspin_double_b2b_1() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::Full, 1, 0, &config, false, 5);
    }

    #[test]
    fn test_3_4_tspin_double_b2b_10() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::Full, 10, 0, &config, false, 5);
    }

    #[test]
    fn test_3_5_tspin_triple_b2b_3() {
        let config = AttackConfig::tetra_league();
        assert_attack(3, SpinType::Full, 3, 0, &config, false, 7);
    }
}

mod combo_multiplier {
    use super::*;

    #[test]
    fn test_4_1_double_combo_1() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::None, 0, 1, &config, false, 1);
    }

    #[test]
    fn test_4_2_double_combo_2() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::None, 0, 2, &config, false, 1);
    }

    #[test]
    fn test_4_3_double_combo_4() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::None, 0, 4, &config, false, 2);
    }

    #[test]
    fn test_4_4_quad_combo_2() {
        let config = AttackConfig::tetra_league();
        assert_attack(4, SpinType::None, 0, 2, &config, false, 6);
    }

    #[test]
    fn test_4_5_quad_combo_5() {
        let config = AttackConfig::tetra_league();
        assert_attack(4, SpinType::None, 0, 5, &config, false, 9);
    }

    #[test]
    fn test_4_6_tspin_double_combo_3() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::Full, 0, 3, &config, false, 7);
    }
}

mod combo_soft_cap {
    use super::*;

    #[test]
    fn test_5_1_single_combo_4() {
        let config = AttackConfig::tetra_league();
        assert_attack(1, SpinType::None, 0, 4, &config, false, 1);
    }

    #[test]
    fn test_5_2_single_combo_8() {
        let config = AttackConfig::tetra_league();
        assert_attack(1, SpinType::None, 0, 8, &config, false, 2);
    }

    #[test]
    fn test_5_3_single_combo_12() {
        let config = AttackConfig::tetra_league();
        assert_attack(1, SpinType::None, 0, 12, &config, false, 2);
    }

    #[test]
    fn test_5_4_single_combo_20() {
        let config = AttackConfig::tetra_league();
        assert_attack(1, SpinType::None, 0, 20, &config, false, 3);
    }
}

mod combined_b2b_combo {
    use super::*;

    #[test]
    fn test_6_1_tspin_double_b2b_2_combo_3() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::Full, 2, 3, &config, false, 8);
    }

    #[test]
    fn test_6_2_quad_b2b_5_combo_4() {
        let config = AttackConfig::tetra_league();
        assert_attack(4, SpinType::None, 5, 4, &config, false, 10);
    }

    #[test]
    fn test_6_3_tspin_triple_b2b_3_combo_2() {
        let config = AttackConfig::tetra_league();
        assert_attack(3, SpinType::Full, 3, 2, &config, false, 10);
    }
}

mod perfect_clear {
    use super::*;

    #[test]
    fn test_7_1_pc_quad_tetra_league() {
        let config = AttackConfig::tetra_league();
        assert_attack(4, SpinType::None, 0, 0, &config, true, 9);
    }

    #[test]
    fn test_7_2_pc_tspin_double_b2b_tetra_league() {
        let config = AttackConfig::tetra_league();
        assert_attack(2, SpinType::Full, 1, 0, &config, true, 11);
    }

    #[test]
    fn test_7_3_pc_quad_quick_play() {
        let config = AttackConfig::quick_play();
        assert_attack(4, SpinType::None, 0, 0, &config, true, 7);
    }
}

mod b2b_surge_tetra_league {
    use super::*;

    #[test]
    fn test_8_1_b2b_before_4() {
        let surge = surge_after_break(4, ChargingConfig::tetra_league());
        assert_eq!(surge, Some([2, 2, 1]));
    }

    #[test]
    fn test_8_2_b2b_before_5() {
        let surge = surge_after_break(5, ChargingConfig::tetra_league());
        assert_eq!(surge, Some([2, 2, 2]));
    }

    #[test]
    fn test_8_3_b2b_before_8() {
        let surge = surge_after_break(8, ChargingConfig::tetra_league());
        assert_eq!(surge, Some([3, 3, 3]));
    }

    #[test]
    fn test_8_4_b2b_before_10() {
        let surge = surge_after_break(10, ChargingConfig::tetra_league());
        assert_eq!(surge, Some([4, 4, 3]));
    }

    #[test]
    fn test_8_5_b2b_before_15() {
        let surge = surge_after_break(15, ChargingConfig::tetra_league());
        assert_eq!(surge, Some([5, 5, 6]));
    }

    #[test]
    fn test_8_6_b2b_before_20() {
        let surge = surge_after_break(20, ChargingConfig::tetra_league());
        assert_eq!(surge, Some([7, 7, 7]));
    }
}

mod b2b_surge_quick_play {
    use super::*;

    #[test]
    fn test_8_7_b2b_before_4() {
        let surge = surge_after_break(4, ChargingConfig::quick_play());
        assert_eq!(surge, Some([1, 1, 0]));
    }

    #[test]
    fn test_8_8_b2b_before_5() {
        let surge = surge_after_break(5, ChargingConfig::quick_play());
        assert_eq!(surge, Some([1, 1, 1]));
    }

    #[test]
    fn test_8_9_b2b_before_8() {
        let surge = surge_after_break(8, ChargingConfig::quick_play());
        assert_eq!(surge, Some([2, 2, 2]));
    }

    #[test]
    fn test_8_10_b2b_before_10() {
        let surge = surge_after_break(10, ChargingConfig::quick_play());
        assert_eq!(surge, Some([3, 3, 2]));
    }
}

mod no_surge {
    use super::*;

    #[test]
    fn test_9_1_b2b_before_0() {
        let surge = surge_after_break(0, ChargingConfig::tetra_league());
        assert_eq!(surge, None);
    }

    #[test]
    fn test_9_2_b2b_before_1() {
        let surge = surge_after_break(1, ChargingConfig::tetra_league());
        assert_eq!(surge, None);
    }

    #[test]
    fn test_9_3_b2b_before_2() {
        let surge = surge_after_break(2, ChargingConfig::tetra_league());
        assert_eq!(surge, None);
    }

    #[test]
    fn test_9_4_b2b_before_3() {
        let surge = surge_after_break(3, ChargingConfig::tetra_league());
        assert_eq!(surge, None);
    }
}

mod spin_detection {
    use super::*;

    fn assert_immobile_spin(piece: Piece) {
        let rotation = Rotation::North;
        let x = 4;
        let y = 1;
        let board = caged_board(piece, rotation, x, y);
        let spin = detect_all_spin(&board, piece, x, y, rotation);
        assert_eq!(spin, SpinType::Mini);
    }

    fn assert_tspin_full(rotation: Rotation) {
        let piece = Piece::T;
        let x = 4;
        let y = 1;
        let board = caged_board(piece, rotation, x, y);
        let spin = detect_all_spin(&board, piece, x, y, rotation);
        assert_eq!(spin, SpinType::Full);
    }

    #[test]
    fn test_10_1_all_mini_i_piece() {
        assert_immobile_spin(Piece::I);
    }

    #[test]
    fn test_10_2_all_mini_o_piece() {
        assert_immobile_spin(Piece::O);
    }

    #[test]
    fn test_10_3_all_mini_t_piece() {
        let rotation = Rotation::North;
        let x = 4;
        let y = 1;
        let mut board = caged_board(Piece::T, rotation, x, y);
        clear_cell(&mut board, x - 1, y + 1);
        clear_cell(&mut board, x + 1, y + 1);
        let spin = detect_all_spin(&board, Piece::T, x, y, rotation);
        assert_eq!(spin, SpinType::Mini);
    }

    #[test]
    fn test_10_4_all_mini_s_piece() {
        assert_immobile_spin(Piece::S);
    }

    #[test]
    fn test_10_5_all_mini_z_piece() {
        assert_immobile_spin(Piece::Z);
    }

    #[test]
    fn test_10_6_all_mini_j_piece() {
        assert_immobile_spin(Piece::J);
    }

    #[test]
    fn test_10_7_all_mini_l_piece() {
        assert_immobile_spin(Piece::L);
    }

    #[test]
    fn test_10_8_tspin_full_north() {
        assert_tspin_full(Rotation::North);
    }

    #[test]
    fn test_10_9_tspin_full_east() {
        assert_tspin_full(Rotation::East);
    }

    #[test]
    fn test_10_10_tspin_full_south() {
        assert_tspin_full(Rotation::South);
    }

    #[test]
    fn test_10_11_tspin_full_west() {
        assert_tspin_full(Rotation::West);
    }
}
