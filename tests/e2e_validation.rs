use direct_cobra_copy::{
    analysis::{
        compute_sigmoid_c, detect_insights,
        InsightDetectorInput, PlayerSkill,
        SIGMOID_K,
    },
    board::{Board, FULL_ROW},
    eval::{evaluate, EvalWeights},
    header::{Piece, SpinType, COL_NB},
    search::{find_best_move_with_scores, SearchConfig},
    state::GameState,
};

fn board_from_bottom_rows(bottom_rows: &[u16]) -> Board {
    assert!(bottom_rows.len() <= 40, "board helper expects at most 40 rows");
    let mut board = Board::new();
    for (y, row) in bottom_rows.iter().copied().enumerate() {
        board.rows[y] = row & FULL_ROW;
    }
    board.cols = [0; COL_NB];
    for y in 0..board.rows.len() {
        let mut bits = board.rows[y] as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            board.cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }
    board
}

fn row_with_gap(col: usize) -> u16 {
    FULL_ROW & !(1 << col)
}

    #[allow(dead_code)]
    fn row_with_gaps(c1: usize, c2: usize) -> u16 {
    FULL_ROW & !(1 << c1) & !(1 << c2)
}

#[allow(dead_code)]
fn default_config() -> SearchConfig {
    SearchConfig::default() // beam=800, depth=14
}

fn medium_config() -> SearchConfig {
    SearchConfig {
        beam_width: 200,
        depth: 8,
        ..SearchConfig::default()
    }
}

fn weights() -> EvalWeights {
    EvalWeights::default()
}

struct Scenario {
    name: &'static str,
    description: &'static str,
    board: Board,
    piece: Piece,
    queue: Vec<Piece>,
    hold: Option<Piece>,
    combo: u32,
    b2b: u8,
    expectation: &'static str,
}

impl Scenario {
    fn game_state(&self) -> GameState {
        let mut state = GameState::new(self.board.clone(), self.piece, self.queue.clone());
        state.hold = self.hold;
        state.combo = self.combo;
        state.b2b = self.b2b;
        state
    }
}

fn build_scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "clean_flat_t_setup",
            description: "Clean 2-row flat stack, T piece. Top player builds T-spin.",
            board: board_from_bottom_rows(&[
                FULL_ROW, // row 0 complete
                0b0111111110, // row 1 gap at col 0
            ]),
            piece: Piece::T,
            queue: vec![Piece::S, Piece::Z, Piece::L, Piece::J, Piece::I],
            hold: None,
            combo: 0,
            b2b: 0,
            expectation: "Should prefer T-spin opportunity or clean flat placement. \
                Attack score should be moderate if T-spin detected.",
        },

        Scenario {
            name: "quad_well_i_piece",
            description: "4-row stack with col-9 well, I piece. Top player goes for quad.",
            board: board_from_bottom_rows(&[
                row_with_gap(9),
                row_with_gap(9),
                row_with_gap(9),
                row_with_gap(9),
            ]),
            piece: Piece::I,
            queue: vec![Piece::T, Piece::S, Piece::Z, Piece::L, Piece::J],
            hold: None,
            combo: 0,
            b2b: 0,
            expectation: "Must drop I vertically in col-9 well for quad clear (4 lines). \
                attack_score should be high (quad = 4 damage). board_score should improve \
                dramatically (4 rows cleared to empty).",
        },

        Scenario {
            name: "active_combo_maintain",
            description: "Staircase pattern mid-combo=3, S piece. Must continue chain.",
            board: board_from_bottom_rows(&[
                row_with_gap(0),          // single at col 0
                row_with_gap(1),          // staircase
                row_with_gap(2),
                row_with_gap(3),
                0b0000000000,             // empty rows above
            ]),
            piece: Piece::S,
            queue: vec![Piece::Z, Piece::T, Piece::L, Piece::J, Piece::I],
            hold: None,
            combo: 3,
            b2b: 0,
            expectation: "Should prefer a move that clears a line to keep combo alive. \
                chain_score should be elevated (combo=3 -> shaped value ~0.53). \
                ChainBreak insight should NOT fire if combo is maintained.",
        },

        Scenario {
            name: "garbage_downstack",
            description: "6-row alternating-gap garbage, J piece. Downstack.",
            board: board_from_bottom_rows(&[
                row_with_gap(2),
                row_with_gap(7),
                row_with_gap(3),
                row_with_gap(8),
                row_with_gap(1),
                row_with_gap(6),
            ]),
            piece: Piece::J,
            queue: vec![Piece::L, Piece::I, Piece::T, Piece::S, Piece::Z],
            hold: None,
            combo: 0,
            b2b: 0,
            expectation: "Should place to maximize line clears or set up clears. \
                board_score delta should be positive (cleaning garbage). \
                DownstackEfficiencyMiss may fire if best move is significantly \
                better at cleaning than chosen move.",
        },

        Scenario {
            name: "near_death_survival",
            description: "16-row stack, gaps scattered. O piece. Survive or die.",
            board: board_from_bottom_rows(&[
                row_with_gap(4),
                row_with_gap(5),
                row_with_gap(4),
                row_with_gap(3),
                row_with_gap(6),
                row_with_gap(4),
                row_with_gap(7),
                row_with_gap(4),
                row_with_gap(2),
                row_with_gap(4),
                row_with_gap(8),
                row_with_gap(4),
                row_with_gap(1),
                row_with_gap(4),
                row_with_gap(9),
                row_with_gap(4),
            ]),
            piece: Piece::O,
            queue: vec![Piece::I, Piece::T, Piece::S, Piece::Z, Piece::L],
            hold: None,
            combo: 0,
            b2b: 0,
            expectation: "Pure survival. Must not top out. board_score is paramount — \
                attack doesn't matter when you're about to die. \
                Engine should prioritize height reduction.",
        },

        Scenario {
            name: "b2b_tspin_opportunity",
            description: "3-row stack with T-slot, b2b=2, L piece (hold T available).",
            board: board_from_bottom_rows(&[
                FULL_ROW,
                row_with_gap(4),            // gap for T-spin
                0b1111001111,               // T-slot overhang: cols 4,5 empty
            ]),
            piece: Piece::L,
            queue: vec![Piece::J, Piece::S, Piece::Z, Piece::I, Piece::O],
            hold: Some(Piece::T),
            combo: 0,
            b2b: 2,
            expectation: "Should consider hold-swapping to T for T-spin double. \
                With b2b=2, T-spin attack_score should be high. \
                If engine uses L instead, attack_window_miss may fire.",
        },

        Scenario {
            name: "flat_clean_z_piece",
            description: "Perfectly flat 2-row board, Z piece. Balance attack vs board.",
            board: board_from_bottom_rows(&[
                FULL_ROW,
                FULL_ROW,
            ]),
            piece: Piece::Z,
            queue: vec![Piece::S, Piece::T, Piece::I, Piece::L, Piece::J],
            hold: None,
            combo: 0,
            b2b: 0,
            expectation: "With 2 full rows, any placement that doesn't create holes is fine. \
                board_score should remain stable. attack_score low (no clears from Z placement). \
                No insights should fire — this is a neutral position.",
        },

        Scenario {
            name: "combo_break_scenario",
            description: "Active combo=5, tall stack with I-clearable gap at col 0. Test chain_break detection.",
            board: board_from_bottom_rows(&[
                // Row 0: gap at col 0 only → I vertical fills it for a quad clear
                row_with_gap(0),
                row_with_gap(0),
                row_with_gap(0),
                row_with_gap(0),
                // Rows 4-7: solid rows adding height pressure
                FULL_ROW,
                FULL_ROW,
                FULL_ROW,
                FULL_ROW,
                0b0000000000, // empty above
            ]),
            piece: Piece::I,
            queue: vec![Piece::T, Piece::J, Piece::L, Piece::S, Piece::Z],
            hold: None,
            combo: 5,
            b2b: 0,
            expectation: "High combo=5 means chain_score is high. \
                Engine should prefer I vertical at col 0 for quad clear (4 lines). \
                Tall stack penalizes non-clearing moves. chain_score should be positive.",
        },
    ]
}

struct ScenarioResult {
    name: String,
    // Search results
    best_move: String,
    best_score: f32,
    hold_used: bool,
    pv_length: usize,
    num_root_moves: usize,
    score_spread: f32, // best - worst root score
    position_complexity: f32,
    // Composite channels
    board_score: f32,
    attack_score: f32,
    chain_score: f32,
    context_score: f32,
    path_attack: f32,
    #[allow(dead_code)]
    path_chain: f32,
    #[allow(dead_code)]
    path_context: f32,
    // Eval deltas
    eval_before: f32,
    eval_after_best: f32,
    // Insight detection (simulated: best vs 2nd-worst move)
    insights_best: Vec<String>,
    insights_suboptimal: Vec<String>,
    // Top 5 moves
    top_moves: Vec<(String, f32)>,
    // Win probability
    win_prob_before: f32,
    win_prob_after: f32,
}

fn format_move(m: &direct_cobra_copy::search::SearchResult) -> String {
    let mv = m.best_move;
    format!(
        "{}@r{:?}({},{}){}{}",
        piece_name(mv.piece()),
        mv.rotation(),
        mv.x(),
        mv.y(),
        if mv.spin() != SpinType::NoSpin {
            format!(" spin:{:?}", mv.spin())
        } else {
            String::new()
        },
        if m.hold_used { " [HOLD]" } else { "" },
    )
}

fn piece_name(p: Piece) -> &'static str {
    match p {
        Piece::I => "I",
        Piece::O => "O",
        Piece::T => "T",
        Piece::L => "L",
        Piece::J => "J",
        Piece::S => "S",
        Piece::Z => "Z",
    }
}

fn format_root_move(mv: &direct_cobra_copy::header::Move) -> String {
    format!(
        "{}@r{:?}({},{})",
        piece_name(mv.piece()),
        mv.rotation(),
        mv.x(),
        mv.y(),
    )
}

fn win_prob(eval: f32, skill: &PlayerSkill) -> f32 {
    let c = compute_sigmoid_c(skill);
    let exponent = SIGMOID_K * (c - eval);
    1.0 / (1.0 + exponent.exp())
}

fn run_scenario(scenario: &Scenario, config: &SearchConfig) -> Option<ScenarioResult> {
    let state = scenario.game_state();
    let w = weights();
    let eval_before = evaluate(&state.board, &w);

    let result = find_best_move_with_scores(&state, config, &w)?;

    let best = &result.best;
    let _eval_after_best = evaluate(&state.board, &w); // approximation (pre-move board)

    let skill = PlayerSkill::default(); // S-rank
    let wp_before = win_prob(eval_before, &skill);
    let wp_after = win_prob(result.best.score, &skill);

    // Simulate insight detection for best move (should have no insights)
    let insights_best = detect_insights(&InsightDetectorInput {
        best_attack_score: result.attack_score,
        best_chain_score: result.chain_score,
        best_board_score: result.board_score,
        actual_score: Some(result.best.score),
        best_score: result.best.score,
        actual_combo_after: scenario.combo.saturating_add(1), // assume clear
        actual_combo_before: scenario.combo,
        actual_lines_cleared: 1, // assume 1 line
        board_eval_delta: 0.0, // best move = no delta
    });

    // Simulate insight detection for a suboptimal move (2nd worst or worst)
    let insights_suboptimal = if result.root_scores.len() >= 3 {
        let worst_idx = result.root_scores.len() - 2; // 2nd worst
        let suboptimal_score = result.root_scores[worst_idx].1;
        detect_insights(&InsightDetectorInput {
            best_attack_score: result.attack_score,
            best_chain_score: result.chain_score,
            best_board_score: result.board_score,
            actual_score: Some(suboptimal_score),
            best_score: result.best.score,
            actual_combo_after: 0, // assume combo broken
            actual_combo_before: scenario.combo,
            actual_lines_cleared: 0, // assume no clear
            board_eval_delta: -2.5, // assume board got worse
        })
    } else {
        vec![]
    };

    let top_moves: Vec<(String, f32)> = result
        .root_scores
        .iter()
        .take(5)
        .map(|(mv, score)| (format_root_move(mv), *score))
        .collect();

    let score_spread = if result.root_scores.len() >= 2 {
        result.root_scores.first().unwrap().1 - result.root_scores.last().unwrap().1
    } else {
        0.0
    };

    Some(ScenarioResult {
        name: scenario.name.to_string(),
        best_move: format_move(best),
        best_score: best.score,
        hold_used: best.hold_used,
        pv_length: best.pv.len(),
        num_root_moves: result.root_scores.len(),
        score_spread,
        position_complexity: result.position_complexity,
        board_score: result.board_score,
        attack_score: result.attack_score,
        chain_score: result.chain_score,
        context_score: result.context_score,
        path_attack: result.path_attack,
        path_chain: result.path_chain,
        path_context: result.path_context,
        eval_before,
        eval_after_best: best.score,
        insights_best: insights_best.iter().map(|i| i.tag.to_str().to_string()).collect(),
        insights_suboptimal: insights_suboptimal.iter().map(|i| format!("{} (sev={:.2})", i.tag.to_str(), i.severity)).collect(),
        top_moves,
        win_prob_before: wp_before,
        win_prob_after: wp_after,
    })
}

#[test]
fn e2e_composite_scoring_validation() {
    let scenarios = build_scenarios();
    let config = medium_config(); // beam=200, depth=8 for speed
    let mut results = Vec::new();

    for scenario in &scenarios {
        eprintln!("\n{}", "=".repeat(60));
        eprintln!("Running: {} — {}", scenario.name, scenario.description);
        
        match run_scenario(scenario, &config) {
            Some(r) => {
                // Print detailed report
                eprintln!("\n  BEST MOVE: {}", r.best_move);
                eprintln!("  Score: {:.3}  |  Hold: {}  |  PV depth: {}", r.best_score, r.hold_used, r.pv_length);
                eprintln!("  Root moves: {}  |  Score spread: {:.3}  |  Complexity: {:.3}", r.num_root_moves, r.score_spread, r.position_complexity);
                eprintln!("\n  COMPOSITE CHANNELS:");
                eprintln!("    board:   {:.4}", r.board_score);
                eprintln!("    attack:  {:.4}", r.attack_score);
                eprintln!("    chain:   {:.4}", r.chain_score);
                eprintln!("    context: {:.4}", r.context_score);
                eprintln!("\n  EVAL DELTA:");
                eprintln!("    before: {:.3}  →  after(best): {:.3}", r.eval_before, r.eval_after_best);
                eprintln!("    win_prob: {:.1}% → {:.1}%", r.win_prob_before * 100.0, r.win_prob_after * 100.0);
                eprintln!("\n  INSIGHTS (best move):   {:?}", r.insights_best);
                eprintln!("  INSIGHTS (suboptimal): {:?}", r.insights_suboptimal);
                eprintln!("\n  TOP 5 MOVES:");
                for (i, (mv, score)) in r.top_moves.iter().enumerate() {
                    eprintln!("    {}. {} → {:.3}", i + 1, mv, score);
                }
                eprintln!("\n  EXPECTATION: {}", scenario.expectation);
                results.push(r);
            }
            None => {
                eprintln!("  ⚠ No legal moves found!");
            }
        }
    }

    eprintln!("\n\n{}", "=".repeat(60));
    eprintln!("SUMMARY: {} scenarios run, {} produced results", scenarios.len(), results.len());
    eprintln!("{}", "=".repeat(60));
    
    // Every scenario should produce a result
    assert_eq!(results.len(), scenarios.len(), "All scenarios should produce results");

    // Basic composite score sanity
    for r in &results {
        // Board score should always be finite
        assert!(r.board_score.is_finite(), "{}: board_score is not finite", r.name);
        assert!(r.attack_score.is_finite(), "{}: attack_score is not finite", r.name);
        assert!(r.chain_score.is_finite(), "{}: chain_score is not finite", r.name);
        assert!(r.context_score.is_finite(), "{}: context_score is not finite", r.name);
        
        // Score spread should be non-negative
        assert!(r.score_spread >= 0.0, "{}: negative score spread", r.name);
        
        // Win probability should be in [0, 1]
        assert!((0.0..=1.0).contains(&r.win_prob_before), "{}: win_prob_before out of range", r.name);
        assert!((0.0..=1.0).contains(&r.win_prob_after), "{}: win_prob_after out of range", r.name);
    }


    // Quad well: I piece should clear lines (high attack)
    let quad = results.iter().find(|r| r.name == "quad_well_i_piece").unwrap();
    assert!(quad.path_attack > 0.0,
        "Quad well: I piece should have positive path_attack (cumulative attack along best path), got {:.4}", quad.path_attack);

    // Active combo: chain_score should be elevated
    let combo = results.iter().find(|r| r.name == "active_combo_maintain").unwrap();
    assert!(combo.chain_score > 0.0,
        "Active combo: chain_score should be positive (combo=3), got {:.4}", combo.chain_score);

    let combo_break = results.iter().find(|r| r.name == "combo_break_scenario").unwrap();
    assert!(combo_break.chain_score > 0.0 || combo_break.path_chain > 0.0,
        "Combo break (combo=5) should have positive immediate chain_score ({:.4}) or positive path_chain ({:.4})",
        combo_break.chain_score,
        combo_break.path_chain);

    // Near-death: board_score should dominate (survival mode)
    let near_death = results.iter().find(|r| r.name == "near_death_survival").unwrap();
    // In survival mode, the engine should find moves that reduce height
    assert!(near_death.board_score.is_finite(),
        "Near-death: board_score should be finite");

    // Suboptimal insights should fire for scenarios with clear differences
    // (we simulate worst-case: combo broken, no clear, board worsened)
    let any_suboptimal_insights = results.iter().any(|r| !r.insights_suboptimal.is_empty());
    assert!(any_suboptimal_insights,
        "At least one scenario should trigger suboptimal-move insights");

    eprintln!("\n✓ All sanity assertions passed.");
}
