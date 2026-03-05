// state.rs -- game state for search with queue support
// extends board::State with piece queue for beam search

use crate::board::Board;
use crate::default_ruleset::ACTIVE_RULES;
use crate::gen::SPAWN_COL;
use crate::header::Piece;
use crate::header::{Move, SpinType};

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
pub enum PhaseState {
    Opener,
    Midgame,
    Endgame,
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
    pub phase: PhaseState,
    pub ply: u32,
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
            phase: PhaseState::Opener,
            ply: 0,
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

        let next_ply = self.ply.saturating_add(1);
        let phase = if next_ply < 8 {
            PhaseState::Opener
        } else if next_ply < 28 {
            PhaseState::Midgame
        } else {
            PhaseState::Endgame
        };

        let _ = obs.resulting_combo;
        let _ = obs.pending_garbage;
        let _ = obs.hold_used;

        Self {
            fatality,
            obligation,
            surge,
            phase,
            ply: next_ply,
        }
    }

    pub fn to_deterministic_string(&self) -> String {
        format!(
            "v2|{}|{}|{}|{}|{}",
            fatality_to_u8(self.fatality),
            obligation_to_u8(self.obligation),
            surge_to_u8(self.surge),
            phase_to_u8(self.phase),
            self.ply,
        )
    }

    pub fn from_deterministic_string(encoded: &str) -> Option<Self> {
        let parts = encoded.split('|').collect::<Vec<_>>();
        if parts.len() != 6 || parts[0] != "v2" {
            return None;
        }

        Some(Self {
            fatality: fatality_from_u8(parts[1].parse().ok()?)?,
            obligation: obligation_from_u8(parts[2].parse().ok()?)?,
            surge: surge_from_u8(parts[3].parse().ok()?)?,
            phase: phase_from_u8(parts[4].parse().ok()?)?,
            ply: parts[5].parse().ok()?,
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

fn phase_to_u8(v: PhaseState) -> u8 {
    match v {
        PhaseState::Opener => 0,
        PhaseState::Midgame => 1,
        PhaseState::Endgame => 2,
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

fn phase_from_u8(v: u8) -> Option<PhaseState> {
    match v {
        0 => Some(PhaseState::Opener),
        1 => Some(PhaseState::Midgame),
        2 => Some(PhaseState::Endgame),
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

    pub fn transition_for_move(
        &self,
        m: &Move,
        lines_cleared: u8,
        hold_used: bool,
        resulting_height: u32,
        spawn_envelope_blocked: bool,
    ) -> CoachingState {
        let (resulting_b2b, resulting_combo) =
            Self::next_chain_values(self.b2b, self.combo, m, lines_cleared);
        let imminent_garbage = self.pending_garbage.saturating_sub(lines_cleared);
        self.coaching.transition(TransitionObservation {
            resulting_height,
            resulting_b2b,
            resulting_combo,
            lines_cleared,
            hold_used,
            pending_garbage: self.pending_garbage,
            imminent_garbage,
            spawn_envelope_blocked,
        })
    }

    pub fn apply_move_transition(
        &mut self,
        m: &Move,
        lines_cleared: u8,
        hold_used: bool,
        resulting_height: u32,
        spawn_envelope_blocked: bool,
    ) {
        let (next_b2b, next_combo) =
            Self::next_chain_values(self.b2b, self.combo, m, lines_cleared);
        let imminent_garbage = self.pending_garbage.saturating_sub(lines_cleared);
        self.b2b = next_b2b;
        self.combo = next_combo;
        self.pending_garbage = imminent_garbage;
        self.coaching = self.coaching.transition(TransitionObservation {
            resulting_height,
            resulting_b2b: next_b2b,
            resulting_combo: next_combo,
            lines_cleared,
            hold_used,
            pending_garbage: self.pending_garbage,
            imminent_garbage,
            spawn_envelope_blocked,
        });
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
        assert_eq!(state.coaching, CoachingState::default());
    }

    #[test]
    fn test_coaching_state_serialization_roundtrip() {
        let state = CoachingState {
            fatality: FatalityState::Critical,
            obligation: ObligationState::MustDownstack,
            surge: SurgeState::Building,
            phase: PhaseState::Midgame,
            ply: 14,
        };

        let encoded = state.to_deterministic_string();
        let decoded = CoachingState::from_deterministic_string(&encoded)
            .unwrap_or_else(|| panic!("failed to decode deterministic string"));

        assert_eq!(decoded, state);
        assert_eq!(encoded, decoded.to_deterministic_string());
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
        assert_eq!(b2b_after_zero, 3, "b2b must be preserved when no lines cleared");
        assert_eq!(combo_after_zero, 0, "combo resets when no lines cleared");

        // Non-difficult line clear (e.g., single/double/triple without spin) resets b2b
        let (b2b_after_single, combo_after_single) = GameState::next_chain_values(3, 4, &m_flat, 1);
        assert_eq!(b2b_after_single, 0, "b2b resets on non-difficult line clear");
        assert_eq!(combo_after_single, 5, "combo increments on any line clear");

        // Quad preserves/increments b2b
        let (b2b_after_quad, combo_after_quad) = GameState::next_chain_values(3, 4, &m_flat, 4);
        assert_eq!(b2b_after_quad, 4, "b2b increments on quad");
        assert_eq!(combo_after_quad, 5, "combo increments on quad");
    }
}
