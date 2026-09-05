use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::Instant;

use crate::attack::{
    calculate_attack_s2_tl, count_cleared_garbage_rows, AttackConfig, S2TlAttackOutcome,
};
use crate::board::{Board, BOARD_HEIGHT};
use crate::eval::EvalWeights;
use crate::header::{Move, Piece, ALL_PIECES, PIECE_NB};
use crate::policy_value_runtime::{PolicyValueRuntime, PolicyValueRuntimeContext};
use crate::search_config::SearchConfig;
use crate::search_expand::{
    reset_search_expansion_stats, search_expansion_stats, set_search_profiling_enabled,
};
use crate::state::GameState;

#[cfg(test)]
thread_local! {
    static PROFILE_SEARCH_DELAY_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static PROFILE_FORCE_SEARCH_NONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// allow: SIZE_OK — P0 plan pins one versus module with 21 in-file named tests.

pub mod report;

// ===== todo 1: rng+bag =====

#[derive(Clone)]
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

    fn next_usize(&mut self, upper: usize) -> usize {
        debug_assert!(upper > 0);
        let n = upper as u64;
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let value = self.next_u64();
            if value < zone {
                return (value % n) as usize;
            }
        }
    }
}

const GAME_SEED_DOMAIN: u64 = 0xD1B5_4A32_D192_ED03;
const PURPOSE_SEED_DOMAIN: u64 = 0xA24B_AED4_963E_E407;
const SLOT_SEED_DOMAIN: u64 = 0x9FB2_1C65_1E98_DF25;

pub(crate) fn stream_seed(match_seed: u64, game: u32, purpose: u8, slot: u8) -> u64 {
    let mut rng = SplitMix64::new(match_seed);
    let mixed = rng.next_u64()
        ^ GAME_SEED_DOMAIN.wrapping_mul(u64::from(game).wrapping_add(1))
        ^ PURPOSE_SEED_DOMAIN.wrapping_mul(u64::from(purpose).wrapping_add(1))
        ^ SLOT_SEED_DOMAIN.wrapping_mul(u64::from(slot).wrapping_add(1));
    SplitMix64::new(mixed).next_u64()
}

#[derive(Clone)]
pub(crate) struct BagStream {
    rng: SplitMix64,
    bag: [Piece; PIECE_NB],
    index: usize,
}

impl BagStream {
    fn new(seed: u64) -> Self {
        let mut stream = Self {
            rng: SplitMix64::new(seed),
            bag: ALL_PIECES,
            index: PIECE_NB,
        };
        stream.refill_bag();
        stream
    }

    fn refill_bag(&mut self) {
        self.bag = ALL_PIECES;
        for i in (1..PIECE_NB).rev() {
            let j = self.rng.next_usize(i + 1);
            self.bag.swap(i, j);
        }
        self.index = 0;
    }

    fn next(&mut self) -> Piece {
        if self.index == PIECE_NB {
            self.refill_bag();
        }
        let piece = self.bag[self.index];
        self.index += 1;
        piece
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn peek_n(&self, n: usize) -> Vec<Piece> {
        let mut clone = self.clone();
        (0..n).map(|_| BagStream::next(&mut clone)).collect()
    }
}

impl Iterator for BagStream {
    type Item = Piece;

    fn next(&mut self) -> Option<Self::Item> {
        Some(BagStream::next(self))
    }
}

pub(crate) struct HoleStream {
    rng: SplitMix64,
}

impl HoleStream {
    fn new(seed: u64) -> Self {
        Self {
            rng: SplitMix64::new(seed),
        }
    }

    fn next(&mut self) -> u8 {
        (self.rng.next_u64() % 10) as u8
    }
}

impl Iterator for HoleStream {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        Some(HoleStream::next(self))
    }
}

// ===== todo 2: garbage queue =====

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GarbageChunk {
    pub rows: u32,
    pub hole: u8,
}

#[derive(Default)]
pub(crate) struct GarbageQueue {
    chunks: VecDeque<GarbageChunk>,
    total: u32,
}

impl GarbageQueue {
    fn new() -> Self {
        Self::default()
    }

    fn enqueue(&mut self, chunk: GarbageChunk) {
        if chunk.rows == 0 {
            return;
        }
        self.total = self.total.saturating_add(chunk.rows);
        self.chunks.push_back(chunk);
    }

    fn cancel(&mut self, rows: u32) -> u32 {
        let mut remaining = rows.min(self.total);
        let cancelled = remaining;
        while remaining > 0 {
            let Some(front) = self.chunks.front_mut() else {
                break;
            };
            if front.rows > remaining {
                front.rows -= remaining;
                self.total -= remaining;
                remaining = 0;
            } else {
                remaining -= front.rows;
                self.total -= front.rows;
                self.chunks.pop_front();
            }
        }
        cancelled
    }

    fn materialize(&mut self, cap: u32) -> Vec<(u32, u8)> {
        let mut remaining = cap.min(self.total);
        let mut materialized = Vec::new();
        while remaining > 0 {
            let Some(front) = self.chunks.front_mut() else {
                break;
            };
            let rows = remaining.min(front.rows);
            let hole = front.hole;
            materialized.push((rows, hole));
            front.rows -= rows;
            self.total -= rows;
            remaining -= rows;
            if front.rows == 0 {
                self.chunks.pop_front();
            }
        }
        materialized
    }

    fn total_rows(&self) -> u32 {
        self.total
    }
}

// ===== todo 3: lock helper =====

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GarbageRows {
    rows: [u64; BOARD_HEIGHT],
}

impl GarbageRows {
    fn new() -> Self {
        Self {
            rows: [0; BOARD_HEIGHT],
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn mark(&mut self, row: usize) {
        self.rows[row] = 1;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn contains(&self, row: usize) -> bool {
        self.rows[row] != 0
    }

    fn materialize_rows(&mut self, rows: u32) {
        let n = usize::try_from(rows)
            .ok()
            .map_or(BOARD_HEIGHT, |value| value.min(BOARD_HEIGHT));
        if n == 0 {
            return;
        }
        if n == BOARD_HEIGHT {
            self.rows = [0; BOARD_HEIGHT];
        } else {
            for y in (n..BOARD_HEIGHT).rev() {
                self.rows[y] = self.rows[y - n];
            }
            for y in 0..n {
                self.rows[y] = 0;
            }
        }
        for y in 0..n {
            self.rows[y] = 1;
        }
    }

    fn apply_clears(&mut self, clears: u64) -> u8 {
        let cleared = count_cleared_garbage_rows(clears, &self.rows);
        if clears == 0 {
            return cleared;
        }
        let mut next = [0u64; BOARD_HEIGHT];
        let mut write = 0usize;
        for read in 0..BOARD_HEIGHT {
            if clears & (1u64 << read) == 0 {
                next[write] = self.rows[read];
                write += 1;
            }
        }
        self.rows = next;
        cleared
    }
}

impl Default for GarbageRows {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LockOutcome {
    pub lines_cleared: u8,
    pub garbage_cleared: u8,
    pub is_pc: bool,
    pub resulting_height: u32,
}

fn lock_piece(board: &mut Board, tracked_garbage: &mut GarbageRows, m: &Move) -> LockOutcome {
    let mech = board.lock(m);
    let garbage_cleared = count_cleared_garbage_rows(mech.cleared_mask, &tracked_garbage.rows);
    if mech.cleared_mask != 0 {
        tracked_garbage.apply_clears(mech.cleared_mask);
    }
    LockOutcome {
        lines_cleared: mech.lines_cleared,
        garbage_cleared,
        is_pc: mech.is_pc,
        resulting_height: mech.resulting_height,
    }
}

// ===== todo 4: S2 chain + transition =====

pub(crate) fn s2_outcome_to_state(outcome: &S2TlAttackOutcome) -> (u8, u32) {
    let b2b = if outcome.b2b_after < 0 {
        0
    } else {
        outcome.b2b_after.min(i32::from(u8::MAX)) as u8
    };
    let combo = if outcome.combo_after < 0 {
        0
    } else {
        outcome.combo_after as u32
    };
    (b2b, combo)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn chain_divergence(
    outcome: &S2TlAttackOutcome,
    next_chain: (u8, u32),
) -> Option<String> {
    let s2 = s2_outcome_to_state(outcome);
    (s2 != next_chain).then(|| format!("s2 chain {:?} diverged from legacy {:?}", s2, next_chain))
}

pub(crate) fn apply_versus_transition(
    state: &mut GameState,
    _m: &Move,
    lock: &LockOutcome,
    s2: (u8, u32),
    inbound_total: u32,
    hold_used: bool,
) {
    let pending = inbound_total.min(u32::from(u8::MAX)) as u8;
    let resulting_height = state.board.height();
    let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&state.board);
    let next = state.chain_state().advance_versus(
        s2,
        lock.lines_cleared,
        pending,
        hold_used,
        resulting_height,
        spawn_envelope_blocked,
    );
    state.set_chain_state(next);
}

// ===== todo 5: piece advance =====

pub(crate) fn advance_piece_state(state: &mut GameState, hold_used: bool, bag: &mut BagStream) {
    let previous_current = state.current;
    if hold_used {
        let hold_was_empty = state.hold.is_none();
        state.hold = Some(previous_current);
        if hold_was_empty && !state.queue.is_empty() {
            state.queue.remove(0);
        }
    }

    state.current = if state.queue.is_empty() {
        bag.next()
    } else {
        state.queue.remove(0)
    };

    while state.queue.len() < 5 {
        state.queue.push(bag.next());
    }
}

// ===== game loop (todo 6) =====

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EngineMode {
    #[default]
    Model,
    Heuristic,
}

pub struct PlayerCfg {
    pub search: SearchConfig,
    pub engine: EngineMode,
    pub label: String,
}

#[derive(Clone, Copy)]
pub struct BaselineSpec {
    pub beam_width: usize,
    pub depth: usize,
    pub clocked_budget_ms: u64,
    pub extend_queue_7bag: bool,
    pub quiescence_max_extensions: usize,
    pub quiescence_beam_fraction: f32,
    pub policy_guided_expansion_cap: usize,
    pub model_path: &'static str,
    pub piece_cap: u32,
    pub ruleset: &'static str,
    pub rng: &'static str,
    pub garbage_model: &'static str,
    pub seed_base: u64,
    pub seed_count: u32,
}

impl BaselineSpec {
    pub fn search_config(self, budget_ms: Option<u64>) -> SearchConfig {
        SearchConfig {
            beam_width: self.beam_width,
            depth: self.depth,
            time_budget_ms: budget_ms,
            extend_queue_7bag: self.extend_queue_7bag,
            attack_config: AttackConfig::tetra_league(),
            quiescence_max_extensions: self.quiescence_max_extensions,
            quiescence_beam_fraction: self.quiescence_beam_fraction,
            policy_guided_expansion_cap: self.policy_guided_expansion_cap,
            nn_scoring: crate::search_config::NnScoringMode::PerChildValue,
            nn_batch: crate::search_config::NnBatchMode::Scalar,
            policy_proxy_weight: crate::search_config::POLICY_BONUS_WEIGHT,
        }
    }
}

pub fn baseline_spec() -> BaselineSpec {
    BaselineSpec {
        beam_width: 800,
        depth: 14,
        clocked_budget_ms: 500,
        extend_queue_7bag: true,
        quiescence_max_extensions: 3,
        quiescence_beam_fraction: 0.15,
        policy_guided_expansion_cap: 32,
        model_path: "models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json",
        piece_cap: 1000,
        ruleset: "production-allspin-const",
        rng: "splitmix64-fisheryates-v1",
        garbage_model: "s2tl-cancel-first-matspawn-cap8-v1",
        seed_base: 20_260_709,
        seed_count: 200,
    }
}

pub fn baseline_player_cfg(label: &str, budget_ms: Option<u64>) -> PlayerCfg {
    PlayerCfg {
        search: baseline_spec().search_config(budget_ms),
        engine: EngineMode::Model,
        label: label.to_owned(),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn baseline_fixture_json_for(search: &SearchConfig, spec: BaselineSpec) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"label\": \"config-frozen baseline\",\n");
    out.push_str("  \"search\": {\n");
    let _ = writeln!(out, "    \"beam_width\": {},", search.beam_width);
    let _ = writeln!(out, "    \"depth\": {},", search.depth);
    match search.time_budget_ms {
        Some(time_budget_ms) => {
            let _ = writeln!(out, "    \"time_budget_ms\": {time_budget_ms},");
        }
        None => out.push_str("    \"time_budget_ms\": null,\n"),
    }
    let _ = writeln!(
        out,
        "    \"extend_queue_7bag\": {},",
        search.extend_queue_7bag
    );
    let _ = writeln!(
        out,
        "    \"quiescence_max_extensions\": {},",
        search.quiescence_max_extensions
    );
    let _ = writeln!(
        out,
        "    \"quiescence_beam_fraction\": {:.4},",
        search.quiescence_beam_fraction
    );
    let _ = writeln!(
        out,
        "    \"policy_guided_expansion_cap\": {},",
        search.policy_guided_expansion_cap
    );
    out.push_str("    \"attack_config\": \"tetra_league\"\n");
    out.push_str("  },\n");
    out.push_str("  \"model\": {\n");
    let _ = writeln!(
        out,
        "    \"path\": \"{}\"",
        report::escape_json_string(spec.model_path)
    );
    out.push_str("  },\n");
    let _ = writeln!(out, "  \"piece_cap\": {},", spec.piece_cap);
    let _ = writeln!(
        out,
        "  \"ruleset\": \"{}\",",
        report::escape_json_string(spec.ruleset)
    );
    let _ = writeln!(
        out,
        "  \"rng\": \"{}\",",
        report::escape_json_string(spec.rng)
    );
    let _ = writeln!(
        out,
        "  \"garbage_model\": \"{}\",",
        report::escape_json_string(spec.garbage_model)
    );
    out.push_str("  \"seeds\": {\n");
    let _ = writeln!(out, "    \"base\": {},", spec.seed_base);
    let _ = writeln!(out, "    \"count\": {}", spec.seed_count);
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn baseline_fixture_json() -> String {
    let spec = baseline_spec();
    baseline_fixture_json_for(&spec.search_config(Some(spec.clocked_budget_ms)), spec)
}

pub(crate) struct PlayerState {
    pub game: GameState,
    pub s2_b2b: i32,
    pub s2_combo: i32,
    pub inbound: GarbageQueue,
    pub tracked_garbage: GarbageRows,
    pub bag: BagStream,
    pub holes: HoleStream,
    pub pieces_locked: u32,
    pub dead: bool,
}

#[derive(Clone, Copy)]
pub struct GameSeeds {
    pub seed: u64,
    pub stream_game_idx: u32,
    pub report_game_idx: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveRecord {
    pub game: u32,
    pub round: u32,
    pub slot: u8,
    pub piece: char,
    pub rot: u8,
    pub x: i8,
    pub y: i8,
    pub spin: String,
    pub hold: bool,
    pub lc: u8,
    pub gc: u8,
    pub atk: u32,
    pub canc: u32,
    pub inb: u32,
    pub b2b: i32,
    pub combo: i32,
    pub ms: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalPhase {
    Alive,
    SpawnTopout,
    SearchNone,
    PieceCap,
}

impl TerminalPhase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::SpawnTopout => "spawn_topout",
            Self::SearchNone => "search_none",
            Self::PieceCap => "piece_cap",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockProfile {
    pub game: u32,
    pub round: u32,
    pub slot: u8,
    pub wall_ns: u64,
    pub returned_move: bool,
    pub piece: Option<char>,
    pub rot: Option<u8>,
    pub x: Option<i8>,
    pub y: Option<i8>,
    pub runtime_attempt_rows: u64,
    pub runtime_unavailable_nodes: u64,
    pub noninferable_nodes: u64,
    pub abandoned_nodes: u64,
    pub runtime_calls: u64,
    pub batch_calls: u64,
    pub inferred_rows: u64,
    pub fallback_levels: u64,
    pub abandoned_levels: u64,
    pub expanded_nodes: u64,
    pub movegen_calls: u64,
    pub infer_nanos: u64,
    pub completed_depth: u32,
    pub completed_width: u32,
    pub deadline_hits: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileEnd {
    pub game: u32,
    pub terminal_slot0: TerminalPhase,
    pub terminal_slot1: TerminalPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameEndReason {
    Win,
    TopOut,
    DrawCap,
    DrawSimul,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameResult {
    pub winner: Option<u8>,
    pub rounds: u32,
    pub pieces: [u32; 2],
    pub reason: GameEndReason,
    pub per_move: Vec<MoveRecord>,
}

fn piece_char(piece: Piece) -> char {
    match piece {
        Piece::I => 'I',
        Piece::O => 'O',
        Piece::T => 'T',
        Piece::L => 'L',
        Piece::J => 'J',
        Piece::S => 'S',
        Piece::Z => 'Z',
    }
}

fn spin_name(mv: &Move) -> &'static str {
    match mv.spin() {
        crate::header::SpinType::NoSpin => "none",
        crate::header::SpinType::Mini => "mini",
        crate::header::SpinType::Full => "full",
    }
}

fn new_player_state(seed: u64, game_idx: u32, slot: u8) -> PlayerState {
    let mut bag = BagStream::new(stream_seed(seed, game_idx, 0, 0));
    let current = bag.next();
    let queue = (0..5).map(|_| bag.next()).collect::<Vec<_>>();
    PlayerState {
        game: GameState::new(Board::new(), current, queue),
        s2_b2b: -1,
        s2_combo: -1,
        inbound: GarbageQueue::new(),
        tracked_garbage: GarbageRows::new(),
        bag,
        holes: HoleStream::new(stream_seed(seed, game_idx, 1, slot)),
        pieces_locked: 0,
        dead: false,
    }
}

fn materialize_player_garbage(player: &mut PlayerState) {
    // 1. MATERIALIZE: pop FIFO chunks from P.inbound — at most 8 rows total
    // this spawn; a partially-consumed chunk keeps its sampled hole column and
    // stays at queue head; one Board::spawn_garbage(rows, hole) call per
    // chunk-part; update P's tracked garbage-row set.
    for (rows, hole) in player.inbound.materialize(8) {
        if let Ok(spawn_rows) = i32::try_from(rows) {
            player.game.board.spawn_garbage(spawn_rows, i32::from(hole));
            player.tracked_garbage.materialize_rows(rows);
        }
    }
}

fn sync_pending_garbage(player: &mut PlayerState) {
    player.game.pending_garbage = player.inbound.total_rows().min(u32::from(u8::MAX)) as u8;
}

fn resolve_lock_exchange(
    att: &mut PlayerState,
    def: &mut PlayerState,
    mv: &Move,
    hold_used: bool,
) -> (LockOutcome, S2TlAttackOutcome) {
    // 5. LOCK: versus lock helper — capture cleared-rows mask BEFORE clearing;
    // garbage_cleared via count_cleared_garbage_rows; is_pc = board empty after
    // clear; shift tracked garbage-row set per cleared rows.
    let lock = lock_piece(&mut att.game.board, &mut att.tracked_garbage, mv);
    // 6. ATTACK: calculate_attack_s2_tl with PRE-move canonical signed counters;
    // update canonical counters verbatim from S2TlAttackOutcome.
    let outcome = calculate_attack_s2_tl(
        lock.lines_cleared,
        mv.spin(),
        att.s2_b2b,
        att.s2_combo,
        lock.is_pc,
        lock.garbage_cleared,
    );
    att.s2_b2b = outcome.b2b_after;
    att.s2_combo = outcome.combo_after;
    // 7. CANCEL→SEND: rem cancels own inbound 1:1; remainder enqueued to
    // OPPONENT as ONE chunk with hole from the RECEIVER's hole stream drawn at
    // enqueue time.
    let cancelled = att.inbound.cancel(outcome.attack);
    let rem = outcome.attack.saturating_sub(cancelled);
    if rem > 0 {
        def.inbound.enqueue(GarbageChunk {
            rows: rem,
            hole: def.holes.next(),
        });
    }
    // 8. BOOKKEEP: apply_versus_transition with post-cancel inbound_total.
    apply_versus_transition(
        &mut att.game,
        mv,
        &lock,
        s2_outcome_to_state(&outcome),
        att.inbound.total_rows(),
        hold_used,
    );
    (lock, outcome)
}

#[cfg(test)]
fn scripted_lock(
    att: &mut PlayerState,
    def: &mut PlayerState,
    mv: &Move,
    hold_used: bool,
) -> (LockOutcome, S2TlAttackOutcome) {
    resolve_lock_exchange(att, def, mv, hold_used)
}

fn elapsed_ms(start: Instant, clocked: bool) -> Option<f64> {
    clocked.then(|| start.elapsed().as_secs_f64() * 1_000.0)
}

fn player_pair(
    players: &mut [PlayerState; 2],
    slot: usize,
) -> (&mut PlayerState, &mut PlayerState) {
    if slot == 0 {
        let (left, right) = players.split_at_mut(1);
        (&mut left[0], &mut right[0])
    } else {
        let (left, right) = players.split_at_mut(1);
        (&mut right[0], &mut left[0])
    }
}

#[derive(Clone, Copy)]
struct TurnContext {
    game_idx: u32,
    round: u32,
    slot: usize,
}

#[derive(Clone, Copy)]
struct SearchEnv<'a> {
    cfg: &'a PlayerCfg,
    model: Option<&'a PolicyValueRuntime>,
    weights: &'a EvalWeights,
}

#[derive(Clone, Copy)]
struct GameEnv<'a> {
    slots: [&'a PlayerCfg; 2],
    model: Option<&'a PolicyValueRuntime>,
    weights: &'a EvalWeights,
    piece_cap: u32,
}

fn process_player_turn(
    ctx: TurnContext,
    env: SearchEnv<'_>,
    players: &mut [PlayerState; 2],
    records: &mut Vec<MoveRecord>,
    record: &mut impl FnMut(MoveRecord),
    profiles: &mut Option<&mut Vec<LockProfile>>,
    terminal: &mut [TerminalPhase; 2],
) {
    let slot_u8 = ctx.slot as u8;
    let (att, def) = player_pair(players, ctx.slot);
    if att.dead {
        return;
    }

    materialize_player_garbage(att);
    // 2. SPAWN CHECK: if spawn_envelope_blocked(&P.board) → P dead this round,
    // skip 3-9. This is the ONLY top-out rule in P0.
    if GameState::spawn_envelope_blocked(&att.game.board) {
        att.dead = true;
        terminal[ctx.slot] = TerminalPhase::SpawnTopout;
        return;
    }
    // 3. SYNC: P.state.pending_garbage = min(P.inbound.total_rows(),255) as u8.
    sync_pending_garbage(att);

    // 4. SEARCH: record wall time — CLOCKED mode only; None result → P dead this round.
    let routed_model = match env.cfg.engine {
        EngineMode::Model => env.model,
        EngineMode::Heuristic => None,
    };
    let runtime_context = routed_model.map(|_| PolicyValueRuntimeContext {
        opponent_board: def.game.board.clone(),
    });
    let profiling = profiles.is_some();
    if profiling {
        reset_search_expansion_stats();
        set_search_profiling_enabled(true);
    }
    let start = Instant::now();
    #[cfg(test)]
    PROFILE_SEARCH_DELAY_MS.with(|delay| {
        let delay_ms = delay.get();
        if profiling && delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
    });
    let search = crate::search::search(
        &att.game,
        &crate::search::SearchRequest {
            config: &env.cfg.search,
            weights: env.weights,
            runtime: routed_model
                .zip(runtime_context.as_ref())
                .map(|(policy_value, context)| crate::search::SearchRuntime {
                    policy_value,
                    context,
                }),
            forced_root_move: None,
        },
    );
    #[cfg(test)]
    let search = if profiling && PROFILE_FORCE_SEARCH_NONE.with(|force| force.get()) {
        None
    } else {
        search
    };
    let ms = elapsed_ms(start, env.cfg.search.time_budget_ms.is_some());
    let wall_ns = if profiling {
        start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    } else {
        0
    };
    let stats = profiling.then(search_expansion_stats);
    if profiling {
        set_search_profiling_enabled(false);
    }
    if let (Some(sink), Some(stats)) = (profiles.as_deref_mut(), stats) {
        let returned = search.as_ref().map(|result| result.best.best_move);
        sink.push(LockProfile {
            game: ctx.game_idx,
            round: ctx.round,
            slot: slot_u8,
            wall_ns,
            returned_move: returned.is_some(),
            piece: returned.map(|mv| piece_char(mv.piece())),
            rot: returned.map(|mv| mv.rotation() as u8),
            x: returned.map(|mv| mv.x() as i8),
            y: returned.map(|mv| mv.y() as i8),
            runtime_attempt_rows: stats.runtime_attempt_rows,
            runtime_unavailable_nodes: stats.runtime_unavailable_nodes,
            noninferable_nodes: stats.noninferable_nodes,
            abandoned_nodes: stats.abandoned_nodes,
            runtime_calls: stats.runtime_calls,
            batch_calls: stats.batch_calls,
            inferred_rows: stats.inferred_rows,
            fallback_levels: stats.fallback_levels,
            abandoned_levels: stats.abandoned_levels,
            expanded_nodes: stats.expanded_nodes,
            movegen_calls: stats.movegen_calls,
            infer_nanos: stats.runtime_inference_nanos,
            completed_depth: stats.completed_depth,
            completed_width: stats.completed_width,
            deadline_hits: stats.deadline_hits,
        });
    }
    let Some(search) = search else {
        att.dead = true;
        terminal[ctx.slot] = TerminalPhase::SearchNone;
        return;
    };

    let mv = search.best.best_move;
    let hold_used = search.best.hold_used;
    let pre_attack_inbound = att.inbound.total_rows();
    let (lock, outcome) = resolve_lock_exchange(att, def, &mv, hold_used);
    let cancelled = pre_attack_inbound.saturating_sub(att.inbound.total_rows());
    att.pieces_locked = att.pieces_locked.saturating_add(1);
    let record_row = MoveRecord {
        game: ctx.game_idx,
        round: ctx.round,
        slot: slot_u8,
        piece: piece_char(mv.piece()),
        rot: mv.rotation() as u8,
        x: mv.x() as i8,
        y: mv.y() as i8,
        spin: spin_name(&mv).to_owned(),
        hold: hold_used,
        lc: lock.lines_cleared,
        gc: lock.garbage_cleared,
        atk: outcome.attack,
        canc: cancelled,
        inb: att.inbound.total_rows(),
        b2b: outcome.b2b_after,
        combo: outcome.combo_after,
        ms,
    };
    records.push(record_row.clone());
    record(record_row);
    // 9. ADVANCE: advance_piece_state with THIS PLAYER'S own bag.
    advance_piece_state(&mut att.game, hold_used, &mut att.bag);
}

fn terminal_result(players: &[PlayerState; 2], rounds: u32, piece_cap: u32) -> Option<GameResult> {
    let pieces = [players[0].pieces_locked, players[1].pieces_locked];
    if players[0].dead && players[1].dead {
        return Some(GameResult {
            winner: None,
            rounds,
            pieces,
            reason: GameEndReason::DrawSimul,
            per_move: Vec::new(),
        });
    }
    if players[0].dead {
        return Some(GameResult {
            winner: Some(1),
            rounds,
            pieces,
            reason: GameEndReason::TopOut,
            per_move: Vec::new(),
        });
    }
    if players[1].dead {
        return Some(GameResult {
            winner: Some(0),
            rounds,
            pieces,
            reason: GameEndReason::TopOut,
            per_move: Vec::new(),
        });
    }
    if players[0].pieces_locked >= piece_cap && players[1].pieces_locked >= piece_cap {
        return Some(GameResult {
            winner: None,
            rounds,
            pieces,
            reason: GameEndReason::DrawCap,
            per_move: Vec::new(),
        });
    }
    None
}

fn run_game_profile_internal(
    game_idx: u32,
    env: GameEnv<'_>,
    players: &mut [PlayerState; 2],
    record: &mut impl FnMut(MoveRecord),
    mut profiles: Option<&mut Vec<LockProfile>>,
) -> (GameResult, ProfileEnd) {
    let mut round = 0u32;
    let mut records = Vec::new();
    let mut terminal = [TerminalPhase::Alive; 2];
    loop {
        process_player_turn(
            TurnContext {
                game_idx,
                round,
                slot: 0,
            },
            SearchEnv {
                cfg: env.slots[0],
                model: env.model,
                weights: env.weights,
            },
            players,
            &mut records,
            record,
            &mut profiles,
            &mut terminal,
        );
        process_player_turn(
            TurnContext {
                game_idx,
                round,
                slot: 1,
            },
            SearchEnv {
                cfg: env.slots[1],
                model: env.model,
                weights: env.weights,
            },
            players,
            &mut records,
            record,
            &mut profiles,
            &mut terminal,
        );
        let completed_rounds = round.saturating_add(1);
        if let Some(mut result) = terminal_result(players, completed_rounds, env.piece_cap) {
            for slot in 0..2 {
                if terminal[slot] == TerminalPhase::Alive
                    && players[slot].pieces_locked >= env.piece_cap
                {
                    terminal[slot] = TerminalPhase::PieceCap;
                }
            }
            result.per_move = records;
            return (
                result,
                ProfileEnd {
                    game: game_idx,
                    terminal_slot0: terminal[0],
                    terminal_slot1: terminal[1],
                },
            );
        }
        round = completed_rounds;
    }
}

fn run_game(
    game_idx: u32,
    slots: [&PlayerCfg; 2],
    model: Option<&PolicyValueRuntime>,
    weights: &EvalWeights,
    piece_cap: u32,
    players: &mut [PlayerState; 2],
    record: &mut impl FnMut(MoveRecord),
) -> GameResult {
    run_game_profile_internal(
        game_idx,
        GameEnv {
            slots,
            model,
            weights,
            piece_cap,
        },
        players,
        record,
        None,
    )
    .0
}

pub fn play_game(
    seed: u64,
    game_idx: u32,
    slots: [&PlayerCfg; 2],
    model: Option<&PolicyValueRuntime>,
    weights: &EvalWeights,
    piece_cap: u32,
    record: &mut impl FnMut(MoveRecord),
) -> GameResult {
    let mut players = [
        new_player_state(seed, game_idx, 0),
        new_player_state(seed, game_idx, 1),
    ];
    run_game(
        game_idx,
        slots,
        model,
        weights,
        piece_cap,
        &mut players,
        record,
    )
}

pub fn play_game_with_stream_index(
    seeds: GameSeeds,
    slots: [&PlayerCfg; 2],
    model: Option<&PolicyValueRuntime>,
    weights: &EvalWeights,
    piece_cap: u32,
    record: &mut impl FnMut(MoveRecord),
) -> GameResult {
    let mut players = [
        new_player_state(seeds.seed, seeds.stream_game_idx, 0),
        new_player_state(seeds.seed, seeds.stream_game_idx, 1),
    ];
    run_game(
        seeds.report_game_idx,
        slots,
        model,
        weights,
        piece_cap,
        &mut players,
        record,
    )
}

pub fn play_game_profiled(
    seeds: GameSeeds,
    slots: [&PlayerCfg; 2],
    model: Option<&PolicyValueRuntime>,
    weights: &EvalWeights,
    piece_cap: u32,
    profiles: &mut Vec<LockProfile>,
    record: &mut impl FnMut(MoveRecord),
) -> (GameResult, ProfileEnd) {
    let mut players = [
        new_player_state(seeds.seed, seeds.stream_game_idx, 0),
        new_player_state(seeds.seed, seeds.stream_game_idx, 1),
    ];
    run_game_profile_internal(
        seeds.report_game_idx,
        GameEnv {
            slots,
            model,
            weights,
            piece_cap,
        },
        &mut players,
        record,
        Some(profiles),
    )
}

// ===== reporting writers (todo 7) =====

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::attack::S2TlAttackOutcome;
    use crate::bag::extend_queue;
    use crate::board::FULL_ROW;
    use crate::header::{Rotation, ALL_PIECES};
    use crate::move_buffer::MoveBuffer;
    use crate::movegen::generate;
    use crate::state::{ObligationState, SurgeState};

    fn baseline_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("bot_arena")
            .join("baseline.json")
    }

    fn board_from_rows(rows: &[u16]) -> Board {
        let mut board = Board::new();
        for (y, row) in rows.iter().copied().enumerate() {
            board.rows[y] = row & FULL_ROW;
            let mut bits = board.rows[y];
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                board.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        board
    }

    fn visible(state: &GameState) -> Vec<Piece> {
        let mut pieces = Vec::with_capacity(6);
        pieces.push(state.current);
        pieces.extend(state.queue.iter().copied());
        pieces
    }

    fn state_from_seed(seed: u64) -> (GameState, BagStream, Vec<Piece>) {
        let mut expected_stream = BagStream::new(seed);
        let expected = (0..32).map(|_| expected_stream.next()).collect::<Vec<_>>();
        let mut bag = BagStream::new(seed);
        let current = bag.next();
        let queue = (0..5).map(|_| bag.next()).collect::<Vec<_>>();
        (GameState::new(Board::new(), current, queue), bag, expected)
    }

    fn assert_cursor_window(state: &GameState, expected: &[Piece], cursor: usize) {
        assert_eq!(visible(state), expected[cursor..cursor + 6]);
        assert_eq!(state.queue.len(), 5);
    }

    fn advance_and_check_cursor(
        state: &mut GameState,
        bag: &mut BagStream,
        expected: &[Piece],
        cursor: &mut usize,
        hold_used: bool,
    ) {
        let first_hold_empty = hold_used && state.hold.is_none();
        advance_piece_state(state, hold_used, bag);
        *cursor += if first_hold_empty { 2 } else { 1 };
        assert_cursor_window(state, expected, *cursor);
    }

    fn splitmix_mul_inverse(value: u64) -> u64 {
        let modulus = 1i128 << 64;
        let mut t = 0i128;
        let mut new_t = 1i128;
        let mut r = modulus;
        let mut new_r = i128::from(value);
        while new_r != 0 {
            let quotient = r / new_r;
            (t, new_t) = (new_t, t - quotient * new_t);
            (r, new_r) = (new_r, r - quotient * new_r);
        }
        if t < 0 {
            t += modulus;
        }
        t as u64
    }

    fn undo_xor_shift_right(mut value: u64, shift: u32) -> u64 {
        let mut step = shift;
        while step < 64 {
            value ^= value >> step;
            step *= 2;
        }
        value
    }

    fn splitmix_seed_for_next_output(output: u64) -> u64 {
        let after_final_xor = undo_xor_shift_right(output, 31);
        let after_second_xor =
            after_final_xor.wrapping_mul(splitmix_mul_inverse(0x94D0_49BB_1331_11EB));
        let after_first_mul = undo_xor_shift_right(after_second_xor, 27);
        let after_first_xor =
            after_first_mul.wrapping_mul(splitmix_mul_inverse(0xBF58_476D_1CE4_E5B9));
        let incremented_state = undo_xor_shift_right(after_first_xor, 30);
        incremented_state.wrapping_sub(0x9E37_79B9_7F4A_7C15)
    }

    #[test]
    fn bag_shuffle_uses_unbiased_rejection_sampling() {
        let seed = splitmix_seed_for_next_output(u64::MAX);
        let mut first = SplitMix64::new(seed);
        assert_eq!(first.next_u64(), u64::MAX);

        let upper = 7usize;
        let n = upper as u64;
        let zone = u64::MAX - (u64::MAX % n);
        let mut reference = SplitMix64::new(seed);
        let expected = loop {
            let value = reference.next_u64();
            if value < zone {
                break (value % n) as usize;
            }
        };
        let expected_next_after_sample = reference.next_u64();

        let mut sampled = SplitMix64::new(seed);
        assert_eq!(sampled.next_usize(upper), expected);
        assert_eq!(sampled.next_u64(), expected_next_after_sample);
    }

    #[test]
    fn bag_stream_deterministic_same_seed() {
        let seed = stream_seed(0x1234_5678_9abc_def0, 0, 0, 0);
        let mut left = BagStream::new(seed);
        let mut right = BagStream::new(seed);
        for _ in 0..140 {
            assert_eq!(left.next(), right.next());
        }
    }

    #[test]
    fn bag_every_7_window_is_permutation() {
        let mut bag = BagStream::new(stream_seed(42, 0, 0, 0));
        let expected = ALL_PIECES.iter().copied().collect::<HashSet<_>>();
        for _ in 0..20 {
            let window = (0..7).map(|_| bag.next()).collect::<HashSet<_>>();
            assert_eq!(window, expected);
        }
    }

    #[test]
    fn bag_streams_differ_across_games_and_purposes() {
        let bag_game0 = BagStream::new(stream_seed(99, 0, 0, 0)).peek_n(21);
        let bag_game1 = BagStream::new(stream_seed(99, 1, 0, 0)).peek_n(21);
        let hole_purpose = BagStream::new(stream_seed(99, 0, 1, 0)).peek_n(21);
        assert_ne!(bag_game0, bag_game1);
        assert_ne!(bag_game0, hole_purpose);
    }

    #[test]
    fn hole_stream_in_range_and_deterministic() {
        let seed = stream_seed(2026, 4, 1, 1);
        let mut left = HoleStream::new(seed);
        let mut right = HoleStream::new(seed);
        for _ in 0..200 {
            let hole = left.next();
            assert!(hole <= 9);
            assert_eq!(hole, right.next());
        }
    }

    #[test]
    fn cancel_first_reduces_inbound_before_sending_attack() {
        let mut inbound = GarbageQueue::new();
        inbound.enqueue(GarbageChunk { rows: 6, hole: 3 });
        inbound.enqueue(GarbageChunk { rows: 4, hole: 8 });
        assert_eq!(inbound.cancel(7), 7);
        assert_eq!(inbound.total_rows(), 3);
        inbound.enqueue(GarbageChunk { rows: 2, hole: 1 });
        assert_eq!(inbound.materialize(8), vec![(3, 8), (2, 1)]);
    }

    #[test]
    fn materialize_caps_eight_rows_and_preserves_remaining_chunks() {
        let mut inbound = GarbageQueue::new();
        inbound.enqueue(GarbageChunk { rows: 10, hole: 4 });
        assert_eq!(inbound.materialize(8), vec![(8, 4)]);
        assert_eq!(inbound.total_rows(), 2);
        assert_eq!(inbound.materialize(8), vec![(2, 4)]);
    }

    #[test]
    fn materialize_uses_fifo_chunk_holes() {
        let mut inbound = GarbageQueue::new();
        inbound.enqueue(GarbageChunk { rows: 5, hole: 2 });
        inbound.enqueue(GarbageChunk { rows: 5, hole: 7 });
        assert_eq!(inbound.materialize(8), vec![(5, 2), (3, 7)]);
        assert_eq!(inbound.materialize(8), vec![(2, 7)]);
    }

    #[test]
    fn inbound_burst_over_255_rows_no_overflow() {
        let mut inbound = GarbageQueue::new();
        inbound.enqueue(GarbageChunk { rows: 300, hole: 6 });
        assert_eq!(inbound.total_rows(), 300);
        let mirror = u8::try_from(inbound.total_rows().min(255)).unwrap_or(u8::MAX);
        assert_eq!(mirror, 255);
    }

    #[test]
    fn lock_counts_garbage_rows_in_cleared_mask() {
        let almost_full = FULL_ROW & !(1u16 << 4) & !(1u16 << 5);
        let mut board = board_from_rows(&[almost_full, almost_full]);
        let mut tracked = GarbageRows::new();
        tracked.mark(0);
        let outcome = lock_piece(
            &mut board,
            &mut tracked,
            &Move::new(Piece::O, Rotation::North, 4, 0, false),
        );
        assert_eq!(outcome.lines_cleared, 2);
        assert_eq!(outcome.garbage_cleared, 1);
    }

    #[test]
    fn lock_detects_perfect_clear() {
        let almost_full = FULL_ROW & !(1u16 << 4) & !(1u16 << 5);
        let mut board = board_from_rows(&[almost_full, almost_full]);
        let mut tracked = GarbageRows::new();
        let outcome = lock_piece(
            &mut board,
            &mut tracked,
            &Move::new(Piece::O, Rotation::North, 4, 0, false),
        );
        assert!(outcome.is_pc);
        assert_eq!(outcome.resulting_height, 0);
    }

    #[test]
    fn lock_matches_board_do_move_on_lines_cleared() {
        let mut rng = SplitMix64::new(0xfeed_face_cafe_beef);
        for i in 0..100 {
            let height = usize::try_from(rng.next_u64() % 8).unwrap_or(0);
            let mut rows = vec![0u16; height];
            for row in &mut rows {
                let hole = u16::try_from(rng.next_u64() % 10).unwrap_or(0);
                *row = u16::try_from(rng.next_u64() & u64::from(FULL_ROW)).unwrap_or(0)
                    & !(1u16 << hole);
            }
            let board = board_from_rows(&rows);
            let piece = ALL_PIECES[i % ALL_PIECES.len()];
            let mut moves = MoveBuffer::new();
            generate(&board, &mut moves, piece, false);
            if let Some(m) = moves.as_slice().first().copied() {
                let mut expected = board.clone();
                let expected_lines = expected.do_move(&m);
                let mut actual = board;
                let mut tracked = GarbageRows::new();
                let outcome = lock_piece(&mut actual, &mut tracked, &m);
                assert_eq!(i32::from(outcome.lines_cleared), expected_lines);
            }
        }
    }

    #[test]
    fn materialize_shifts_existing_garbage_row_tracking_upward() {
        let mut tracked = GarbageRows::new();
        tracked.mark(0);
        tracked.mark(1);
        tracked.materialize_rows(3);
        for row in 0..5 {
            assert!(tracked.contains(row));
        }
        assert_eq!(tracked.apply_clears(1u64 << 2), 1);
        assert!(tracked.contains(0));
        assert!(tracked.contains(1));
        assert!(tracked.contains(2));
        assert!(tracked.contains(3));
        assert!(!tracked.contains(4));

        let mut shifted = GarbageRows::new();
        shifted.mark(2);
        assert_eq!(shifted.apply_clears(1), 0);
        assert!(shifted.contains(1));
    }

    #[test]
    fn s2_chain_override_clamps_to_gamestate_types() {
        let negative = S2TlAttackOutcome {
            attack: 0,
            b2b_after: -1,
            combo_after: -1,
        };
        assert_eq!(s2_outcome_to_state(&negative), (0, 0));
        let high = S2TlAttackOutcome {
            attack: 0,
            b2b_after: 300,
            combo_after: 42,
        };
        assert_eq!(s2_outcome_to_state(&high), (255, 42));
        let agreement = S2TlAttackOutcome {
            attack: 0,
            b2b_after: 1,
            combo_after: 1,
        };
        assert!(chain_divergence(&agreement, (1, 1)).is_none());
    }

    #[test]
    fn coaching_follows_s2_chain_on_divergence() {
        let mut state = GameState::new(Board::new(), Piece::I, vec![Piece::O; 5]);
        let lock = LockOutcome {
            lines_cleared: 1,
            garbage_cleared: 0,
            is_pc: false,
            resulting_height: 0,
        };
        let m = Move::new(Piece::I, Rotation::North, 3, 0, false);
        apply_versus_transition(&mut state, &m, &lock, (3, 1), 0, false);
        assert_eq!(state.b2b, 3);
        assert_eq!(state.coaching.surge, SurgeState::Active);
        assert!(chain_divergence(
            &S2TlAttackOutcome {
                attack: 0,
                b2b_after: 3,
                combo_after: 1
            },
            GameState::next_chain_values(0, 0, &m, 1)
        )
        .is_some());
    }

    #[test]
    fn versus_transition_advances_bag_counters_once() {
        let mut state = GameState::new(Board::new(), Piece::I, vec![Piece::O; 5]);
        state.pieces_into_bag = 6;
        let lock = LockOutcome {
            lines_cleared: 2,
            garbage_cleared: 0,
            is_pc: false,
            resulting_height: 0,
        };
        let m = Move::new(Piece::O, Rotation::North, 4, 0, false);
        apply_versus_transition(&mut state, &m, &lock, (0, 1), 9, true);
        assert_eq!(state.bag_number, 1);
        assert_eq!(state.pieces_into_bag, 0);
        assert_eq!(state.lines_total, 2);
        assert_eq!(state.pending_garbage, 9);
    }

    #[test]
    fn observation_garbage_reflects_post_cancel_queue() {
        let m = Move::new(Piece::I, Rotation::North, 3, 0, false);
        let lock = LockOutcome {
            lines_cleared: 0,
            garbage_cleared: 0,
            is_pc: false,
            resulting_height: 0,
        };
        let mut canceled = GameState::new(Board::new(), Piece::I, vec![Piece::O; 5]);
        canceled.pending_garbage = 9;
        apply_versus_transition(&mut canceled, &m, &lock, (0, 0), 0, false);
        assert_ne!(canceled.coaching.obligation, ObligationState::MustCancel);
        assert_ne!(canceled.coaching.obligation, ObligationState::MustDownstack);

        let mut inbound = GameState::new(Board::new(), Piece::I, vec![Piece::O; 5]);
        apply_versus_transition(&mut inbound, &m, &lock, (0, 0), 6, false);
        assert_eq!(inbound.coaching.obligation, ObligationState::MustCancel);
        assert_eq!(inbound.pending_garbage, 6);
    }

    #[test]
    fn advance_no_hold() {
        let (mut state, mut bag, expected) = state_from_seed(7);
        advance_piece_state(&mut state, false, &mut bag);
        assert_eq!(visible(&state), expected[1..7]);
        assert!(state.hold.is_none());
    }

    #[test]
    fn advance_first_hold_empty() {
        let (mut state, mut bag, expected) = state_from_seed(8);
        let original_current = state.current;
        advance_piece_state(&mut state, true, &mut bag);
        assert_eq!(state.hold, Some(original_current));
        assert_eq!(visible(&state), expected[2..8]);
    }

    #[test]
    fn advance_swap_hold() {
        let (mut state, mut bag, expected) = state_from_seed(9);
        let original_current = state.current;
        state.hold = Some(Piece::Z);
        advance_piece_state(&mut state, true, &mut bag);
        assert_eq!(state.hold, Some(original_current));
        assert_eq!(visible(&state), expected[1..7]);
    }

    #[test]
    fn advance_strong_cursor_invariant_mixed_walk() {
        let seed = 12;
        let mut expected_stream = BagStream::new(seed);
        let expected = (0..64).map(|_| expected_stream.next()).collect::<Vec<_>>();
        let mut bag = BagStream::new(seed);
        let current = bag.next();
        let queue = (0..5).map(|_| bag.next()).collect::<Vec<_>>();
        let mut state = GameState::new(Board::new(), current, queue);
        let mut cursor = 0usize;
        assert_cursor_window(&state, &expected, cursor);

        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, false);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, false);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, true);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, false);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, true);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, true);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, false);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, false);
        advance_and_check_cursor(&mut state, &mut bag, &expected, &mut cursor, true);
    }

    #[test]
    fn queue_refills_to_five() {
        let seed = 10;
        let mut expected_stream = BagStream::new(seed);
        let expected = (0..16).map(|_| expected_stream.next()).collect::<Vec<_>>();
        let mut bag = BagStream::new(seed);
        let current = bag.next();
        let queue = vec![bag.next()];
        let mut state = GameState::new(Board::new(), current, queue);
        advance_piece_state(&mut state, false, &mut bag);
        assert_eq!(state.queue.len(), 5);
        assert_eq!(visible(&state), expected[1..7]);
    }

    #[test]
    fn bag_preview_stays_consistent_across_boundary_with_extend_queue() {
        let seed = stream_seed(11, 0, 0, 0);
        let mut bag = BagStream::new(seed);
        let first_bag = bag.peek_n(7);
        let current = bag.next();
        let queue = (0..5).map(|_| bag.next()).collect::<Vec<_>>();
        let state = GameState::new(Board::new(), current, queue.clone());
        let extended = extend_queue(&state.queue, state.current, state.hold);
        let suffix = extended[state.queue.len()..]
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let consumed = visible(&state).into_iter().collect::<HashSet<_>>();
        let missing = first_bag
            .into_iter()
            .filter(|piece| !consumed.contains(piece))
            .collect::<HashSet<_>>();
        assert_eq!(suffix, missing);
    }

    fn tiny_cfg() -> PlayerCfg {
        PlayerCfg {
            search: crate::search_config::SearchConfig {
                beam_width: 16,
                depth: 2,
                time_budget_ms: None,
                extend_queue_7bag: true,
                ..crate::search_config::SearchConfig::default()
            },
            engine: EngineMode::Model,
            label: "tiny".to_owned(),
        }
    }

    #[test]
    fn attempt_row_identity_holds_per_lock() {
        let cfg = tiny_cfg();
        let mut profiles = Vec::new();

        let _ = play_game_profiled(
            GameSeeds {
                seed: 42,
                stream_game_idx: 0,
                report_game_idx: 0,
            },
            [&cfg, &cfg],
            None,
            &EvalWeights::default(),
            2,
            &mut profiles,
            &mut |_| {},
        );

        assert!(!profiles.is_empty());
        for profile in profiles {
            assert_eq!(profile.runtime_attempt_rows, 0);
            assert_eq!(profile.abandoned_nodes, 0);
            assert_eq!(
                profile.runtime_unavailable_nodes + profile.noninferable_nodes,
                profile.expanded_nodes
            );
        }

        let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json");
        if !model_path.exists() {
            return;
        }
        let runtime = PolicyValueRuntime::load(&model_path).expect("checked-in model should load");
        let model_cfg = PlayerCfg {
            search: SearchConfig {
                beam_width: 4,
                depth: 1,
                time_budget_ms: None,
                extend_queue_7bag: false,
                ..SearchConfig::default()
            },
            engine: EngineMode::Model,
            label: "model-identity".to_owned(),
        };
        let mut model_profiles = Vec::new();
        let _ = play_game_profiled(
            GameSeeds {
                seed: 43,
                stream_game_idx: 0,
                report_game_idx: 1,
            },
            [&model_cfg, &model_cfg],
            Some(&runtime),
            &EvalWeights::default(),
            1,
            &mut model_profiles,
            &mut |_| {},
        );
        assert!(!model_profiles.is_empty());
        for profile in model_profiles {
            assert_eq!(
                profile.runtime_attempt_rows
                    + profile.runtime_unavailable_nodes
                    + profile.noninferable_nodes
                    + profile.abandoned_nodes,
                profile.expanded_nodes
            );
            assert_eq!(profile.inferred_rows, profile.runtime_attempt_rows);
        }
    }

    #[test]
    #[ignore = "model-inference tier (~10s each): cargo test -- --ignored"]
    fn mixed_engine_routes_runtime_per_side() {
        let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json");
        if !model_path.exists() {
            return;
        }
        let runtime = PolicyValueRuntime::load(&model_path).expect("checked-in model should load");
        let mut model_cfg = tiny_cfg();
        model_cfg.label = "A".to_owned();
        model_cfg.engine = EngineMode::Model;
        model_cfg.search.nn_scoring = crate::search_config::NnScoringMode::PolicyProxy;
        model_cfg.search.nn_batch = crate::search_config::NnBatchMode::Level;
        let mut heuristic_cfg = tiny_cfg();
        heuristic_cfg.label = "B".to_owned();
        heuristic_cfg.engine = EngineMode::Heuristic;
        heuristic_cfg.search.nn_scoring = crate::search_config::NnScoringMode::PolicyProxy;
        heuristic_cfg.search.nn_batch = crate::search_config::NnBatchMode::Level;
        let mut profiles = Vec::new();

        for (game, slots) in [
            (0, [&model_cfg, &heuristic_cfg]),
            (1, [&heuristic_cfg, &model_cfg]),
        ] {
            let _ = play_game_profiled(
                GameSeeds {
                    seed: 77_000_002,
                    stream_game_idx: 0,
                    report_game_idx: game,
                },
                slots,
                Some(&runtime),
                &EvalWeights::default(),
                1,
                &mut profiles,
                &mut |_| {},
            );
        }

        let mut model_rows = 0usize;
        let mut heuristic_rows = 0usize;
        for profile in profiles {
            let a_slot = profile.game % 2;
            if u32::from(profile.slot) == a_slot {
                model_rows += 1;
                assert!(profile.runtime_attempt_rows > 0);
            } else {
                heuristic_rows += 1;
                assert_eq!(profile.runtime_attempt_rows, 0);
                assert_eq!(profile.runtime_calls, 0);
                assert_eq!(profile.batch_calls, 0);
                assert_eq!(profile.inferred_rows, 0);
                assert!(profile.runtime_unavailable_nodes > 0);
            }
        }
        assert!(model_rows > 0);
        assert!(heuristic_rows > 0);
    }

    #[test]
    fn lock_profile_wall_ns_measured_around_search() {
        let cfg = tiny_cfg();
        let mut profiles = Vec::new();
        PROFILE_SEARCH_DELAY_MS.with(|delay| delay.set(50));

        let _ = play_game_profiled(
            GameSeeds {
                seed: 42,
                stream_game_idx: 0,
                report_game_idx: 0,
            },
            [&cfg, &cfg],
            None,
            &EvalWeights::default(),
            1,
            &mut profiles,
            &mut |_| {},
        );
        PROFILE_SEARCH_DELAY_MS.with(|delay| delay.set(0));

        assert!(profiles.iter().all(|profile| profile.wall_ns >= 45_000_000));
    }

    #[test]
    fn failed_search_attempt_recorded_before_death() {
        let cfg = tiny_cfg();
        let mut players = [new_player_state(42, 0, 0), new_player_state(42, 0, 1)];
        let mut records = Vec::new();
        let mut profiles = Vec::new();
        let mut profile_sink = Some(&mut profiles);
        let mut terminal = [TerminalPhase::Alive; 2];
        PROFILE_FORCE_SEARCH_NONE.with(|force| force.set(true));

        process_player_turn(
            TurnContext {
                game_idx: 0,
                round: 0,
                slot: 0,
            },
            SearchEnv {
                cfg: &cfg,
                model: None,
                weights: &EvalWeights::default(),
            },
            &mut players,
            &mut records,
            &mut |_| {},
            &mut profile_sink,
            &mut terminal,
        );
        PROFILE_FORCE_SEARCH_NONE.with(|force| force.set(false));

        assert_eq!(profiles.len(), 1);
        assert!(!profiles[0].returned_move);
        assert!(profiles[0].wall_ns > 0);
        assert_eq!(terminal[0], TerminalPhase::SearchNone);
        let jsonl = report::profile_jsonl(
            &profiles,
            &[ProfileEnd {
                game: 0,
                terminal_slot0: terminal[0],
                terminal_slot1: terminal[1],
            }],
        );
        let rows = jsonl
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSONL"))
            .collect::<Vec<_>>();
        assert_eq!(rows[0]["returned_move"], false);
        assert_eq!(rows[1]["terminal_slot0"], "search_none");
    }

    #[test]
    fn profile_jsonl_populations_reconcile() {
        let cfg = tiny_cfg();
        let mut profiles = Vec::new();
        let (result, end) = play_game_profiled(
            GameSeeds {
                seed: 42,
                stream_game_idx: 0,
                report_game_idx: 0,
            },
            [&cfg, &cfg],
            None,
            &EvalWeights::default(),
            2,
            &mut profiles,
            &mut |_| {},
        );
        let ends = [end];

        let successful = profiles
            .iter()
            .filter(|profile| profile.returned_move)
            .map(|profile| {
                (
                    profile.game,
                    profile.round,
                    profile.slot,
                    profile.piece,
                    profile.rot,
                    profile.x,
                    profile.y,
                )
            })
            .collect::<Vec<_>>();
        let replay_moves = result
            .per_move
            .iter()
            .map(|record| {
                (
                    record.game,
                    record.round,
                    record.slot,
                    Some(record.piece),
                    Some(record.rot),
                    Some(record.x),
                    Some(record.y),
                )
            })
            .collect::<Vec<_>>();
        let jsonl = report::profile_jsonl(&profiles, &ends);
        let rows = jsonl
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSONL"))
            .collect::<Vec<_>>();
        let lock_rows = rows.iter().filter(|row| row["t"] == "lock").count();
        let end_rows = rows.iter().filter(|row| row["t"] == "end").count();

        assert_eq!(successful, replay_moves);
        assert_eq!(end_rows, 1);
        assert_eq!(lock_rows, profiles.len());
        assert_eq!(rows.len(), profiles.len() + ends.len());
        assert!(profiles
            .iter()
            .filter(|profile| !profile.returned_move)
            .all(|profile| ends.iter().any(|end| {
                end.game == profile.game
                    && match profile.slot {
                        0 => end.terminal_slot0 == TerminalPhase::SearchNone,
                        1 => end.terminal_slot1 == TerminalPhase::SearchNone,
                        _ => false,
                    }
            })));
    }

    fn first_generated_move(board: &Board, piece: Piece, lines: u8) -> Move {
        let mut moves = MoveBuffer::new();
        generate(board, &mut moves, piece, false);
        for mv in moves.as_slice().iter().copied() {
            let mut candidate = board.clone();
            if candidate.do_move(&mv) == i32::from(lines) {
                return mv;
            }
        }
        panic!("missing generated move for {piece:?} clearing {lines}");
    }

    fn almost_full_rows(count: usize) -> Board {
        let row = FULL_ROW & !(1u16 << 4);
        board_from_rows(&vec![row; count])
    }

    fn almost_full_rows_with_tail(count: usize) -> Board {
        let mut rows = vec![FULL_ROW & !(1u16 << 4); count];
        rows.push(1u16 << 0);
        board_from_rows(&rows)
    }

    fn locked_piece_for_scripted_hold(state: &GameState, hold_used: bool) -> Piece {
        if !hold_used {
            return state.current;
        }
        state
            .hold
            .or_else(|| state.queue.first().copied())
            .unwrap_or(state.current)
    }

    fn expected_locked_sequence(seed: u64, holds: &[bool]) -> Vec<Piece> {
        let mut stream = BagStream::new(seed).peek_n(holds.len() + 12);
        let mut current = stream.remove(0);
        let mut queue = stream.drain(0..5).collect::<Vec<_>>();
        let mut hold = None;
        let mut locked = Vec::with_capacity(holds.len());

        for hold_used in holds.iter().copied() {
            locked.push(if hold_used {
                hold.unwrap_or(queue[0])
            } else {
                current
            });
            if hold_used {
                let previous_current = current;
                let hold_was_empty = hold.is_none();
                hold = Some(previous_current);
                if hold_was_empty {
                    queue.remove(0);
                }
            }
            current = if queue.is_empty() {
                stream.remove(0)
            } else {
                queue.remove(0)
            };
            while queue.len() < 5 {
                queue.push(stream.remove(0));
            }
        }

        locked
    }

    #[test]
    fn simultaneous_topout_is_draw() {
        let cfg = tiny_cfg();
        let mut players = [new_player_state(42, 0, 0), new_player_state(42, 0, 1)];
        players[0]
            .inbound
            .enqueue(GarbageChunk { rows: 24, hole: 0 });
        players[1]
            .inbound
            .enqueue(GarbageChunk { rows: 24, hole: 9 });
        let mut records = Vec::new();

        let result = run_game(
            0,
            [&cfg, &cfg],
            None,
            &crate::eval::EvalWeights::default(),
            20,
            &mut players,
            &mut |record| records.push(record),
        );

        assert_eq!(result.winner, None);
        assert_eq!(result.reason, GameEndReason::DrawSimul);
    }

    #[test]
    fn garbage_high_but_envelope_open_is_alive() {
        let mut player = new_player_state(7, 0, 0);
        player.game.board = board_from_rows(&[(1u16 << 0) | (1u16 << 9); 30]);
        player.inbound.enqueue(GarbageChunk { rows: 8, hole: 5 });

        materialize_player_garbage(&mut player);

        assert!(!GameState::spawn_envelope_blocked(&player.game.board));
        assert!(!player.dead);
    }

    #[test]
    fn piece_cap_reached_is_draw() {
        let cfg = tiny_cfg();
        let mut records = Vec::new();

        let result = play_game(
            42,
            0,
            [&cfg, &cfg],
            None,
            &crate::eval::EvalWeights::default(),
            20,
            &mut |record| records.push(record),
        );

        assert_eq!(result.winner, None);
        assert_eq!(result.reason, GameEndReason::DrawCap);
        assert_eq!(result.pieces, [20, 20]);
    }

    #[test]
    fn tiny_deterministic_game_completes() {
        let cfg = tiny_cfg();
        let mut first_records = Vec::new();
        let first = play_game(
            42,
            0,
            [&cfg, &cfg],
            None,
            &crate::eval::EvalWeights::default(),
            20,
            &mut |record| first_records.push(record),
        );
        let mut second_records = Vec::new();
        let second = play_game(
            42,
            0,
            [&cfg, &cfg],
            None,
            &crate::eval::EvalWeights::default(),
            20,
            &mut |record| second_records.push(record),
        );

        assert_eq!(first, second);
        assert_eq!(first_records, second_records);
        assert!(first.per_move.iter().all(|record| record.ms.is_none()));
    }

    #[test]
    fn first_mover_garbage_lands_same_round() {
        let cfg = tiny_cfg();
        let mut players = [new_player_state(99, 0, 0), new_player_state(99, 0, 1)];
        players[0].game.current = Piece::I;
        players[0].game.board = almost_full_rows(4);
        players[1].game.current = Piece::O;
        let mut records = Vec::new();

        let _ = run_game(
            0,
            [&cfg, &cfg],
            None,
            &crate::eval::EvalWeights::default(),
            1,
            &mut players,
            &mut |record| records.push(record),
        );

        assert!(records
            .iter()
            .any(|record| record.slot == 0 && record.atk >= 4));
        assert!(players[1].tracked_garbage.contains(0));
    }

    #[test]
    fn both_slots_lock_identical_piece_sequences() {
        let seed = 2026;
        let game_idx = 3;
        let holds = [true, false, true, false, true, false, true];
        let expected = expected_locked_sequence(stream_seed(seed, game_idx, 0, 0), &holds);
        let mut players = [
            new_player_state(seed, game_idx, 0),
            new_player_state(seed, game_idx, 1),
        ];
        let mut locked = [Vec::new(), Vec::new()];

        for hold_used in holds {
            for slot in 0..2 {
                let piece = locked_piece_for_scripted_hold(&players[slot].game, hold_used);
                players[slot].game.board = Board::new();
                let mv = first_generated_move(&players[slot].game.board, piece, 0);
                let (att, def) = player_pair(&mut players, slot);
                let (lock, outcome) = scripted_lock(att, def, &mv, hold_used);
                assert_eq!(lock.lines_cleared, 0);
                assert_eq!(outcome.attack, 0);
                att.pieces_locked = att.pieces_locked.saturating_add(1);
                advance_piece_state(&mut att.game, hold_used, &mut att.bag);
                locked[slot].push(piece);
            }
        }

        assert!(locked[0].len() >= 6);
        assert_eq!(locked[0], locked[1]);
        assert_eq!(locked[0], expected);
        assert_eq!(
            players[0].game.pieces_into_bag,
            players[1].game.pieces_into_bag
        );
        assert_eq!(players[0].game.bag_number, players[1].game.bag_number);
    }

    #[test]
    fn baseline_fixture_matches_code_constants() {
        let expected = baseline_fixture_json();
        let actual = fs::read_to_string(baseline_fixture_path())
            .expect("baseline fixture should exist on disk");

        assert_eq!(actual, expected);

        let actual_value: serde_json::Value =
            serde_json::from_str(&actual).expect("fixture should parse as json");
        let expected_value: serde_json::Value =
            serde_json::from_str(&expected).expect("expected baseline should parse as json");
        assert_eq!(actual_value, expected_value);
    }

    #[test]
    fn baseline_v1_defaults_pin_new_search_fields() {
        let config = baseline_spec().search_config(None);

        assert_eq!(
            config.nn_scoring,
            crate::search_config::NnScoringMode::PerChildValue
        );
        assert_eq!(config.nn_batch, crate::search_config::NnBatchMode::Scalar);
        assert_eq!(config.policy_proxy_weight.to_bits(), 0.10f32.to_bits());
    }

    #[test]
    fn game_init_advances_bag_cursor_past_visible_window() {
        let seed = stream_seed(404, 0, 0, 0);
        let expected = BagStream::new(seed).peek_n(8);
        let mut player = new_player_state(404, 0, 0);

        advance_piece_state(&mut player.game, false, &mut player.bag);

        assert_eq!(visible(&player.game)[5], expected[6]);
    }

    #[test]
    fn attack_continuity_no_clear_then_single() {
        let mut att = new_player_state(1, 0, 0);
        let mut def = new_player_state(1, 0, 1);
        att.game.current = Piece::O;
        let no_clear = first_generated_move(&att.game.board, Piece::O, 0);
        let (_, no_clear_attack) = scripted_lock(&mut att, &mut def, &no_clear, false);
        assert_eq!(no_clear_attack.attack, 0);
        assert_eq!(att.s2_b2b, -1);
        assert_eq!(att.s2_combo, -1);

        att.game.current = Piece::I;
        att.game.board = almost_full_rows(1);
        let single = first_generated_move(&att.game.board, Piece::I, 1);
        let (_, single_attack) = scripted_lock(&mut att, &mut def, &single, false);
        assert_eq!(single_attack.attack, 0);
        assert_eq!(att.s2_b2b, -1);
        assert_eq!(att.s2_combo, 0);
    }

    #[test]
    fn attack_continuity_combo_chain() {
        let mut att = new_player_state(2, 0, 0);
        let mut def = new_player_state(2, 0, 1);
        let mut attacks = Vec::new();
        let mut combos = Vec::new();
        for _ in 0..3 {
            att.game.current = Piece::I;
            att.game.board = almost_full_rows(1);
            let single = first_generated_move(&att.game.board, Piece::I, 1);
            let (_, outcome) = scripted_lock(&mut att, &mut def, &single, false);
            attacks.push(outcome.attack);
            combos.push(att.s2_combo);
            assert_eq!(att.s2_b2b, -1);
        }

        assert_eq!(attacks, vec![0, 0, 1]);
        assert_eq!(combos, vec![0, 1, 2]);
    }

    #[test]
    fn attack_continuity_b2b_break_then_quad() {
        let mut att = new_player_state(3, 0, 0);
        let mut def = new_player_state(3, 0, 1);

        att.game.current = Piece::I;
        att.game.board = almost_full_rows_with_tail(4);
        let quad = first_generated_move(&att.game.board, Piece::I, 4);
        let (_, first) = scripted_lock(&mut att, &mut def, &quad, false);
        assert_eq!(first.attack, 4);
        assert_eq!((att.s2_b2b, att.s2_combo), (0, 0));

        att.game.current = Piece::I;
        att.game.board = almost_full_rows(1);
        let single = first_generated_move(&att.game.board, Piece::I, 1);
        let (_, second) = scripted_lock(&mut att, &mut def, &single, false);
        assert_eq!(second.attack, 0);
        assert_eq!((att.s2_b2b, att.s2_combo), (-1, 1));

        att.game.current = Piece::I;
        att.game.board = almost_full_rows_with_tail(4);
        let quad = first_generated_move(&att.game.board, Piece::I, 4);
        let (_, third) = scripted_lock(&mut att, &mut def, &quad, false);
        assert_eq!(third.attack, 6);
        assert_eq!((att.s2_b2b, att.s2_combo), (0, 2));
    }

    #[test]
    fn search_none_impossible_board_topout_probe() {
        let cfg = tiny_cfg();
        let mut players = [new_player_state(88, 0, 0), new_player_state(88, 0, 1)];
        let mut rows = [FULL_ROW; BOARD_HEIGHT];
        rows[19] = 0;
        rows[20] = 0;
        players[0].game.board = board_from_rows(&rows);
        let mut records = Vec::new();

        let result = run_game(
            0,
            [&cfg, &cfg],
            None,
            &crate::eval::EvalWeights::default(),
            3,
            &mut players,
            &mut |record| records.push(record),
        );

        println!("search_none_probe={:?}", result.reason);
        assert_eq!(result.reason, GameEndReason::TopOut);
        assert_eq!(result.winner, Some(1));
    }
}
