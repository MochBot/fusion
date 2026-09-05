// state.rs -- game state for search with queue support

use crate::attack::{calculate_attack_s2_tl_with_multiplier, AttackConfig};
use crate::board::{Board, LockMechanics};
use crate::default_ruleset::ACTIVE_RULES;
use crate::gen::SPAWN_COL;
use crate::header::Piece;
use crate::header::{Move, SpinType};

/// The seven bookkeeping fields a lock advances, detached from board and
/// queue so search nodes, the versus sim, and the wasm sequence sim can all
/// share one transition rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChainState {
    pub b2b: u8,
    pub combo: u32,
    pub pending_garbage: u8,
    pub lines_total: u32,
    pub bag_number: u32,
    pub pieces_into_bag: u8,
    pub coaching: CoachingState,
}

pub struct LockTransition {
    pub chain: ChainState,
    pub attack: f32,
    pub clear_event: Option<ClearEvent>,
}

impl ChainState {
    #[allow(clippy::too_many_arguments)]
    fn advance_core(
        &self,
        next_b2b: u8,
        next_combo: u32,
        observed_pending: u8,
        imminent_garbage: u8,
        lines_cleared: u8,
        hold_used: bool,
        resulting_height: u32,
        spawn_envelope_blocked: bool,
    ) -> ChainState {
        let next_pieces_into_bag = (self.pieces_into_bag + 1) % 7;
        let bag_number = if self.pieces_into_bag == 6 {
            self.bag_number.saturating_add(1)
        } else {
            self.bag_number
        };
        ChainState {
            b2b: next_b2b,
            combo: next_combo,
            pending_garbage: imminent_garbage,
            lines_total: self.lines_total.saturating_add(lines_cleared as u32),
            bag_number,
            pieces_into_bag: next_pieces_into_bag,
            coaching: self.coaching.transition(TransitionObservation {
                resulting_height,
                resulting_b2b: next_b2b,
                resulting_combo: next_combo,
                lines_cleared,
                hold_used,
                pending_garbage: observed_pending,
                imminent_garbage,
                spawn_envelope_blocked,
            }),
        }
    }

    /// Coaching-family lock transition on the S2/TL attack formula — the
    /// same rule the coaching gap line, versus exchange, and training labels
    /// use. Unsigned chain counters store `signed_s2 + 1` (0 = no active
    /// chain, matching the legacy `GameState` meaning). Without a tracked
    /// garbage-row mask, `garbage_cleared` uses the pending-clear heuristic
    /// (`push_expand_record`'s convention when no mask is supplied).
    pub fn advance_lock(
        &self,
        m: &Move,
        lock: &LockMechanics,
        hold_used: bool,
        spawn_envelope_blocked: bool,
        attack_config: &AttackConfig,
    ) -> LockTransition {
        let lines_cleared = lock.lines_cleared;
        let imminent_garbage = self.pending_garbage.saturating_sub(lines_cleared);
        let clears_garbage = self.pending_garbage > 0 && lines_cleared > 0;

        let outcome = calculate_attack_s2_tl_with_multiplier(
            lines_cleared,
            m.spin(),
            i32::from(self.b2b) - 1,
            self.combo as i32 - 1,
            lock.is_pc,
            u8::from(clears_garbage),
            f64::from(attack_config.garbage_multiplier),
        );
        let next_b2b = outcome.b2b_after.saturating_add(1).clamp(0, 255) as u8;
        let next_combo = outcome.combo_after.saturating_add(1).max(0) as u32;
        let attack = outcome.attack as f32;
        let is_surge_release = lines_cleared > 0 && next_b2b == 0 && self.b2b >= 5;

        let clear_event = (lines_cleared > 0).then(|| ClearEvent {
            clear_type: ClearType::from_lines(lines_cleared),
            spin_type: m.spin(),
            lines_cleared,
            attack_sent: attack,
            b2b_before: self.b2b,
            b2b_after: next_b2b,
            combo_before: self.combo,
            combo_after: next_combo,
            is_surge_release,
            is_garbage_clear: clears_garbage,
            is_perfect_clear: lock.is_pc,
            piece: m.piece(),
        });

        LockTransition {
            chain: self.advance_core(
                next_b2b,
                next_combo,
                self.pending_garbage,
                imminent_garbage,
                lines_cleared,
                hold_used,
                lock.resulting_height,
                spawn_envelope_blocked,
            ),
            attack,
            clear_event,
        }
    }

    /// Versus-family lock transition: chain counters come from the S2/TL
    /// attack outcome and pending garbage from the post-cancel inbound
    /// queue; only the shared bookkeeping (bag counters, lines total,
    /// coaching observation) is computed here.
    pub fn advance_versus(
        &self,
        s2_chain: (u8, u32),
        lines_cleared: u8,
        pending_after_cancel: u8,
        hold_used: bool,
        resulting_height: u32,
        spawn_envelope_blocked: bool,
    ) -> ChainState {
        self.advance_core(
            s2_chain.0,
            s2_chain.1,
            pending_after_cancel,
            pending_after_cancel,
            lines_cleared,
            hold_used,
            resulting_height,
            spawn_envelope_blocked,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatalityState {
    Safe,
    Critical,
    Fatal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObligationState {
    None,
    MustDownstack,
    MustCancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurgeState {
    Dormant,
    Building,
    Active,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearType {
    None,
    Single,
    Double,
    Triple,
    Quad,
    Penta,
}

impl ClearType {
    pub fn from_lines(lines: u8) -> Self {
        match lines {
            0 => ClearType::None,
            1 => ClearType::Single,
            2 => ClearType::Double,
            3 => ClearType::Triple,
            4 => ClearType::Quad,
            _ => ClearType::Penta,
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            ClearType::None => "none",
            ClearType::Single => "single",
            ClearType::Double => "double",
            ClearType::Triple => "triple",
            ClearType::Quad => "quad",
            ClearType::Penta => "penta",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClearEvent {
    pub clear_type: ClearType,
    pub spin_type: SpinType,
    pub lines_cleared: u8,
    pub attack_sent: f32,
    pub b2b_before: u8,
    pub b2b_after: u8,
    pub combo_before: u32,
    pub combo_after: u32,
    pub is_surge_release: bool,
    pub is_garbage_clear: bool,
    pub is_perfect_clear: bool,
    pub piece: Piece,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoachingState {
    pub fatality: FatalityState,
    pub obligation: ObligationState,
    pub surge: SurgeState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransitionObservation {
    pub(crate) resulting_height: u32,
    pub(crate) resulting_b2b: u8,
    pub(crate) resulting_combo: u32,
    pub(crate) lines_cleared: u8,
    pub(crate) hold_used: bool,
    pub(crate) pending_garbage: u8,
    pub(crate) imminent_garbage: u8,
    pub(crate) spawn_envelope_blocked: bool,
}

impl Default for CoachingState {
    fn default() -> Self {
        Self {
            fatality: FatalityState::Safe,
            obligation: ObligationState::None,
            surge: SurgeState::Dormant,
        }
    }
}

impl CoachingState {
    pub(crate) fn transition(&self, obs: TransitionObservation) -> Self {
        let fatality = if obs.spawn_envelope_blocked || obs.resulting_height >= 35 {
            FatalityState::Fatal
        } else if obs.resulting_height >= 28 {
            FatalityState::Critical
        } else {
            FatalityState::Safe
        };

        let obligation = if matches!(fatality, FatalityState::Fatal)
            || (obs.imminent_garbage >= 3 && obs.lines_cleared == 0)
        {
            ObligationState::MustCancel
        } else if obs.resulting_height >= 26
            || (obs.imminent_garbage >= 1 && obs.lines_cleared == 0)
        {
            ObligationState::MustDownstack
        } else {
            ObligationState::None
        };

        let surge = if obs.resulting_b2b >= 3 {
            SurgeState::Active
        } else if obs.resulting_b2b >= 1 {
            SurgeState::Building
        } else {
            SurgeState::Dormant
        };

        let _ = obs.resulting_combo;
        let _ = obs.pending_garbage;
        let _ = obs.hold_used;

        Self {
            fatality,
            obligation,
            surge,
        }
    }

    pub fn to_deterministic_string(&self) -> String {
        format!(
            "v3|{}|{}|{}",
            fatality_to_u8(self.fatality),
            obligation_to_u8(self.obligation),
            surge_to_u8(self.surge),
        )
    }

    pub fn from_deterministic_string(encoded: &str) -> Option<Self> {
        let parts = encoded.split('|').collect::<Vec<_>>();
        if parts.len() != 4 || parts[0] != "v3" {
            return None;
        }

        Some(Self {
            fatality: fatality_from_u8(parts[1].parse().ok()?)?,
            obligation: obligation_from_u8(parts[2].parse().ok()?)?,
            surge: surge_from_u8(parts[3].parse().ok()?)?,
        })
    }
}

fn fatality_to_u8(v: FatalityState) -> u8 {
    match v {
        FatalityState::Safe => 0,
        FatalityState::Critical => 1,
        FatalityState::Fatal => 2,
    }
}

fn obligation_to_u8(v: ObligationState) -> u8 {
    match v {
        ObligationState::None => 0,
        ObligationState::MustDownstack => 1,
        ObligationState::MustCancel => 2,
    }
}

fn surge_to_u8(v: SurgeState) -> u8 {
    match v {
        SurgeState::Dormant => 0,
        SurgeState::Building => 1,
        SurgeState::Active => 2,
    }
}

fn fatality_from_u8(v: u8) -> Option<FatalityState> {
    match v {
        0 => Some(FatalityState::Safe),
        1 => Some(FatalityState::Critical),
        2 => Some(FatalityState::Fatal),
        _ => None,
    }
}

fn obligation_from_u8(v: u8) -> Option<ObligationState> {
    match v {
        0 => Some(ObligationState::None),
        1 => Some(ObligationState::MustDownstack),
        2 => Some(ObligationState::MustCancel),
        _ => None,
    }
}

fn surge_from_u8(v: u8) -> Option<SurgeState> {
    match v {
        0 => Some(SurgeState::Dormant),
        1 => Some(SurgeState::Building),
        2 => Some(SurgeState::Active),
        _ => None,
    }
}

/// game state carrying everything the search needs
#[derive(Clone)]
pub struct GameState {
    pub board: Board,
    pub current: Piece,
    pub hold: Option<Piece>,
    pub queue: Vec<Piece>,
    pub b2b: u8, // surge level (0 = no B2B chain)
    pub combo: u32,
    pub pending_garbage: u8,
    pub lines_total: u32,
    pub bag_number: u32,
    pub pieces_into_bag: u8,
    pub coaching: CoachingState,
}

impl GameState {
    pub fn new(board: Board, current: Piece, queue: Vec<Piece>) -> Self {
        Self {
            board,
            current,
            hold: None,
            queue,
            b2b: 0,
            combo: 0,
            pending_garbage: 0,
            lines_total: 0,
            bag_number: 0,
            pieces_into_bag: 0,
            coaching: CoachingState::default(),
        }
    }

    /// next piece from queue, or None if exhausted
    pub fn queue_piece(&self, index: usize) -> Option<Piece> {
        self.queue.get(index).copied()
    }

    /// how many pieces remain in queue
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn infer_hold_used_for_piece(&self, piece: Piece) -> bool {
        if self.hold == Some(piece) {
            return true;
        }
        self.hold.is_none() && self.queue.first().copied() == Some(piece) && piece != self.current
    }

    pub fn spawn_envelope_blocked(board: &Board) -> bool {
        let spawn_y = ACTIVE_RULES.spawn_row;
        if spawn_y < 0 {
            return false;
        }

        let pivot_x = SPAWN_COL as i32;
        let envelope = [
            (pivot_x - 1, spawn_y),
            (pivot_x, spawn_y),
            (pivot_x + 1, spawn_y),
            (pivot_x + 2, spawn_y),
            (pivot_x - 1, spawn_y + 1),
            (pivot_x, spawn_y + 1),
            (pivot_x + 1, spawn_y + 1),
        ];

        envelope
            .iter()
            .any(|(x, y)| board.obstructed(*x, *y) || board.occupied(*x, *y))
    }

    pub fn next_chain_values(
        current_b2b: u8,
        current_combo: u32,
        m: &Move,
        lines_cleared: u8,
    ) -> (u8, u32) {
        if lines_cleared == 0 {
            return (current_b2b, 0);
        }

        let next_b2b = if m.spin() != SpinType::NoSpin || lines_cleared == 4 {
            current_b2b.saturating_add(1)
        } else {
            0
        };
        let next_combo = current_combo.saturating_add(1);
        (next_b2b, next_combo)
    }

    pub fn chain_state(&self) -> ChainState {
        ChainState {
            b2b: self.b2b,
            combo: self.combo,
            pending_garbage: self.pending_garbage,
            lines_total: self.lines_total,
            bag_number: self.bag_number,
            pieces_into_bag: self.pieces_into_bag,
            coaching: self.coaching,
        }
    }

    pub fn set_chain_state(&mut self, chain: ChainState) {
        self.b2b = chain.b2b;
        self.combo = chain.combo;
        self.pending_garbage = chain.pending_garbage;
        self.lines_total = chain.lines_total;
        self.bag_number = chain.bag_number;
        self.pieces_into_bag = chain.pieces_into_bag;
        self.coaching = chain.coaching;
    }

    pub fn apply_move_transition(
        &mut self,
        m: &Move,
        lines_cleared: u8,
        hold_used: bool,
        resulting_height: u32,
        spawn_envelope_blocked: bool,
    ) {
        let mechanics = LockMechanics {
            cleared_mask: 0,
            lines_cleared,
            is_pc: false,
            resulting_height,
        };
        let next = self
            .chain_state()
            .advance_lock(
                m,
                &mechanics,
                hold_used,
                spawn_envelope_blocked,
                &AttackConfig::tetra_league(),
            )
            .chain;
        self.set_chain_state(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{Move, Rotation};
    use crate::movegen::{generate, MoveBuffer};

    #[test]
    fn test_gamestate_creation() {
        let board = Board::new();
        let state = GameState::new(board, Piece::T, vec![Piece::I, Piece::O, Piece::S]);
        assert_eq!(state.current, Piece::T);
        assert!(state.hold.is_none());
        assert_eq!(state.queue_len(), 3);
        assert_eq!(state.queue_piece(0), Some(Piece::I));
        assert_eq!(state.queue_piece(2), Some(Piece::S));
        assert_eq!(state.queue_piece(5), None);
        assert_eq!(state.b2b, 0);
        assert_eq!(state.combo, 0);
        assert_eq!(state.pending_garbage, 0);
        assert_eq!(state.lines_total, 0);
        assert_eq!(state.bag_number, 0);
        assert_eq!(state.pieces_into_bag, 0);
        assert_eq!(state.coaching, CoachingState::default());
    }

    #[test]
    fn test_coaching_state_serialization_roundtrip() {
        let state = CoachingState {
            fatality: FatalityState::Critical,
            obligation: ObligationState::MustDownstack,
            surge: SurgeState::Building,
        };

        let encoded = state.to_deterministic_string();
        let decoded = CoachingState::from_deterministic_string(&encoded)
            .unwrap_or_else(|| panic!("failed to decode deterministic string"));

        assert_eq!(decoded, state);
        assert_eq!(encoded, decoded.to_deterministic_string());
        assert_eq!(encoded, "v3|1|1|1");
        assert_eq!(
            CoachingState::from_deterministic_string("v2|1|1|1|1|14"),
            None
        );
    }

    #[test]
    fn test_transition_determinism_and_reconstruction() {
        let mut state_a =
            GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O, Piece::L]);
        let mut state_b = state_a.clone();

        let mut snapshots_a = Vec::new();
        let mut snapshots_b = Vec::new();

        for _ in 0..4 {
            let mut moves_a = MoveBuffer::new();
            generate(&state_a.board, &mut moves_a, state_a.current, false);
            let selected_a = moves_a.as_slice()[0];
            let mut board_after_a = state_a.board.clone();
            let lines_a = board_after_a.do_move(&selected_a) as u8;
            let height_a = board_after_a.height();
            state_a.apply_move_transition(&selected_a, lines_a, false, height_a, false);
            state_a.board = board_after_a;
            state_a.current = state_a.queue_piece(0).unwrap_or(Piece::I);
            snapshots_a.push(state_a.coaching.to_deterministic_string());

            let mut moves_b = MoveBuffer::new();
            generate(&state_b.board, &mut moves_b, state_b.current, false);
            let selected_b = moves_b.as_slice()[0];
            let mut board_after_b = state_b.board.clone();
            let lines_b = board_after_b.do_move(&selected_b) as u8;
            let height_b = board_after_b.height();
            state_b.apply_move_transition(&selected_b, lines_b, false, height_b, false);
            state_b.board = board_after_b;
            state_b.current = state_b.queue_piece(0).unwrap_or(Piece::I);
            snapshots_b.push(state_b.coaching.to_deterministic_string());
        }

        assert_eq!(
            snapshots_a, snapshots_b,
            "transition sequence must be deterministic"
        );

        for snapshot in snapshots_a {
            let reconstructed = CoachingState::from_deterministic_string(&snapshot)
                .unwrap_or_else(|| panic!("failed to reconstruct state"));
            assert_eq!(snapshot, reconstructed.to_deterministic_string());
        }
    }

    #[test]
    fn test_transition_obligation_and_fatality_thresholds() {
        let base = CoachingState::default();
        let next = base.transition(TransitionObservation {
            resulting_height: 36,
            resulting_b2b: 0,
            resulting_combo: 0,
            lines_cleared: 0,
            hold_used: false,
            pending_garbage: 6,
            imminent_garbage: 6,
            spawn_envelope_blocked: false,
        });

        assert_eq!(next.fatality, FatalityState::Fatal);
        assert_eq!(next.obligation, ObligationState::MustCancel);

        let downstack_case = base.transition(TransitionObservation {
            resulting_height: 27,
            resulting_b2b: 1,
            resulting_combo: 1,
            lines_cleared: 0,
            hold_used: false,
            pending_garbage: 0,
            imminent_garbage: 1,
            spawn_envelope_blocked: false,
        });

        assert_eq!(downstack_case.fatality, FatalityState::Safe);
        assert_eq!(downstack_case.obligation, ObligationState::MustDownstack);
    }

    #[test]
    fn test_next_chain_values() {
        let m_tspin = Move::new_tspin(Rotation::North, 4, 0, true);
        let (b2b_after_tspin, combo_after_tspin) = GameState::next_chain_values(2, 3, &m_tspin, 2);
        assert_eq!(b2b_after_tspin, 3);
        assert_eq!(combo_after_tspin, 4);

        let m_flat = Move::new(Piece::I, Rotation::North, 4, 0, false);
        let (b2b_after_zero, combo_after_zero) = GameState::next_chain_values(3, 4, &m_flat, 0);
        assert_eq!(
            b2b_after_zero, 3,
            "b2b must be preserved when no lines cleared"
        );
        assert_eq!(combo_after_zero, 0, "combo resets when no lines cleared");

        // Non-difficult line clear (e.g., single/double/triple without spin) resets b2b
        let (b2b_after_single, combo_after_single) = GameState::next_chain_values(3, 4, &m_flat, 1);
        assert_eq!(
            b2b_after_single, 0,
            "b2b resets on non-difficult line clear"
        );
        assert_eq!(combo_after_single, 5, "combo increments on any line clear");

        // Quad preserves/increments b2b
        let (b2b_after_quad, combo_after_quad) = GameState::next_chain_values(3, 4, &m_flat, 4);
        assert_eq!(b2b_after_quad, 4, "b2b increments on quad");
        assert_eq!(combo_after_quad, 5, "combo increments on quad");
    }
}
