//! Replay recommendation wire data and normalization.

use crate::coach_beam::{CoachLineResult, CoachLineStep};
use crate::header::{piece_from_external, piece_to_external, Move, Piece};
use crate::search::SearchResultFull;
use crate::state::GameState;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const RECOMMEND_CONTRACT_VERSION: u32 = 1;
pub const BOARD_ROWS: usize = 40;
const ROW_MASK_10BIT: u16 = (1u16 << 10) - 1;

/// Known source identities and identifiers supplied by other registered implementations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendSourceId {
    #[serde(rename = "s2-fixed")]
    S2Fixed,
    #[serde(rename = "human-prior")]
    HumanPrior,
    #[serde(rename = "cached")]
    Cached,
    #[serde(rename = "native-search")]
    NativeSearch,
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for RecommendSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            RecommendSourceId::S2Fixed => "s2-fixed",
            RecommendSourceId::HumanPrior => "human-prior",
            RecommendSourceId::Cached => "cached",
            RecommendSourceId::NativeSearch => "native-search",
            RecommendSourceId::Custom(id) => id,
        };
        f.write_str(s)
    }
}

/// Completion of the returned path relative to the requested continuation.
/// A partial path is never presented as complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecommendCompletion {
    Complete,
    Partial,
    Empty,
}

/// Why a request produced no candidate. Every variant stays distinguishable
/// on the wire: unsupported conditioning, cache misses, absent models,
/// disabled execution, illegal continuations, and execution failures are
/// different facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecommendUnavailability {
    NoLegalContinuation,
    CacheMiss,
    ModelAbsent,
    ExecutionDisabled,
    UnsupportedConditioning,
    UnsupportedInput,
}

impl fmt::Display for RecommendUnavailability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            RecommendUnavailability::NoLegalContinuation => "no-legal-continuation",
            RecommendUnavailability::CacheMiss => "cache-miss",
            RecommendUnavailability::ModelAbsent => "model-absent",
            RecommendUnavailability::ExecutionDisabled => "execution-disabled",
            RecommendUnavailability::UnsupportedConditioning => "unsupported-conditioning",
            RecommendUnavailability::UnsupportedInput => "unsupported-input",
        };
        f.write_str(s)
    }
}

/// Full search position (the engine may choose hold/queue) versus a fixed
/// continuation sequence (pieces are played in the supplied order). A fixed
/// sequence is not a full hold search and must not be declared as one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RecommendInput {
    FullPosition {
        hold_supported: bool,
        queue_extension_7bag: bool,
    },
    FixedSequence {
        extend_beyond_supplied: bool,
        hold_supported: bool,
    },
}

/// Requested conditioning. No retained implementation consumes rank or
/// personal history yet, so any present requirement is rejected instead of
/// silently dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendConditioning {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
}

/// The supplied situation: canonical boards, external piece IDs, and the
/// chain seed the mechanics assume.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendSituation {
    /// 40 y-up 10-bit rows. Length-checked in `validate_request`; serde
    /// only implements arrays up to 32, so this stays a `Vec`.
    pub board_rows: Vec<u16>,
    pub gmask: Vec<u32>,
    pub pieces: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<u8>,
    pub b2b: i32,
    pub combo: i32,
    pub pending: i32,
    pub mult: f64,
    pub beam_width: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendRequest {
    pub version: u32,
    pub source: RecommendSourceId,
    pub config: String,
    pub input: RecommendInput,
    pub situation: RecommendSituation,
    #[serde(default)]
    pub conditioning: RecommendConditioning,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<RecommendOptions>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_lambda: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dig_combo_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dig_attack_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spin_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_grace: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b2b_floor: Option<f64>,
}

/// One placement on the candidate path. `piece` is the external ID.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendPlacement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_score: Option<f64>,
    pub piece: u8,
    pub rotation: u8,
    pub x: i8,
    pub y: i8,
    pub spin: u8,
    pub hold_used: bool,
}

/// Mechanical evidence for one path step. Every field is optional: unknown
/// stays absent, never zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendMechanics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows_after: Option<Vec<u16>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_attack: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surge_delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b2b_after: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo_after: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b2b_before: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo_before: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_surge_release: Option<bool>,
}

/// Supplemental algorithm-specific scores. These are diagnostics attached to
/// a candidate, never a cross-engine ranking and never candidates
/// themselves. `policy_logit` is the raw logit, never a probability.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendScores {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_composite: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_logit: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub root_scores: Vec<RecommendRootScore>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendRootScore {
    pub piece: u8,
    pub rotation: u8,
    pub x: i8,
    pub y: i8,
    pub spin: u8,
    pub score: f32,
}

/// The assumptions the generating adapter actually used.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendAssumptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub garbage_model: Option<RecommendGarbageModel>,
    pub hold_supported: bool,
    pub queue_extension_7bag: bool,
    pub seed_b2b: i32,
    pub seed_combo: i32,
    pub seed_pending: i32,
    pub garbage_mult: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecommendGarbageModel {
    OriginMask,
    PendingClearHeuristic,
    StoredEvidence,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendCandidate {
    pub source: RecommendSourceId,
    pub config: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    pub path: Vec<RecommendPlacement>,
    pub completion: RecommendCompletion,
    pub requested_plies: usize,
    pub completed_plies: usize,
    pub mechanics: Vec<RecommendMechanics>,
    /// Shaped selection total (raw attack plus surge-potential change).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaped_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_used: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<RecommendEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_total: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surge_total: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_rows: Option<Vec<u16>>,
    /// Absent when the producing beam does not track it (e.g. the WASM
    /// fixed-continuation line), never a zero array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_gmask: Option<Vec<u32>>,
    #[serde(default)]
    pub scores: RecommendScores,
    pub assumptions: RecommendAssumptions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
pub enum RecommendOutcome {
    Ready { candidate: Box<RecommendCandidate> },
    Unavailable { reason: RecommendUnavailability },
    Failed { error: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendEvidence {
    pub id: String,
    pub provider: String,
    pub final_height: u32,
    pub final_holes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines_cleared: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_end_overhangs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_descriptor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_completeness: Option<String>,
}

mod native;
pub use native::{recommend_json, recommend_native};

/// Parse an untrusted outcome at the boundary. Returns a serde error on
/// malformed input; well-formed but inapplicable requests are
/// `Unavailable`, never a parse failure.
pub fn parse_recommend_outcome(
    value: &serde_json::Value,
) -> Result<RecommendOutcome, serde_json::Error> {
    let outcome = serde_json::from_value(value.clone())?;
    if let RecommendOutcome::Ready { candidate } = &outcome {
        validate_candidate(candidate).map_err(<serde_json::Error as serde::de::Error>::custom)?;
    }
    Ok(outcome)
}

/// Parse an untrusted candidate at the boundary.
pub fn parse_recommend_candidate(
    value: &serde_json::Value,
) -> Result<RecommendCandidate, serde_json::Error> {
    let candidate = serde_json::from_value(value.clone())?;
    validate_candidate(&candidate).map_err(<serde_json::Error as serde::de::Error>::custom)?;
    Ok(candidate)
}

fn validate_candidate(candidate: &RecommendCandidate) -> Result<(), &'static str> {
    if candidate.source.to_string().is_empty()
        || candidate.completed_plies != candidate.path.len()
        || candidate.mechanics.len() != candidate.path.len()
        || candidate.completion != completion_of(candidate.requested_plies, candidate.path.len())
    {
        return Err("candidate completion and mechanics must match its path");
    }
    if candidate
        .path
        .iter()
        .any(|mv| mv.piece > 6 || mv.rotation > 3 || mv.spin > 2)
    {
        return Err("invalid placement");
    }
    for rows in candidate.final_rows.iter().chain(
        candidate
            .mechanics
            .iter()
            .filter_map(|step| step.rows_after.as_ref()),
    ) {
        if rows.len() != BOARD_ROWS || rows.iter().any(|row| row & !ROW_MASK_10BIT != 0) {
            return Err("invalid board rows");
        }
    }
    if candidate.final_gmask.as_ref().is_some_and(|rows| {
        rows.len() != BOARD_ROWS || rows.iter().any(|row| row & !u32::from(ROW_MASK_10BIT) != 0)
    }) {
        return Err("invalid garbage mask");
    }
    Ok(())
}

/// Validate a request against what retained implementations support.
/// Unsupported rank/history conditioning, unknown pieces, malformed boards,
/// and version skew are rejected with an explicit reason.
pub fn validate_request(request: &RecommendRequest) -> Result<(), RecommendUnavailability> {
    if request.version != RECOMMEND_CONTRACT_VERSION {
        return Err(RecommendUnavailability::UnsupportedInput);
    }
    if request.source.to_string().is_empty() {
        return Err(RecommendUnavailability::UnsupportedInput);
    }
    if request.conditioning.rank.is_some() || request.conditioning.history.is_some() {
        return Err(RecommendUnavailability::UnsupportedConditioning);
    }
    for row in request.situation.board_rows.iter().copied() {
        if row & !ROW_MASK_10BIT != 0 {
            return Err(RecommendUnavailability::UnsupportedInput);
        }
    }
    if request.situation.board_rows.len() != BOARD_ROWS
        || request.situation.gmask.len() != BOARD_ROWS
    {
        return Err(RecommendUnavailability::UnsupportedInput);
    }
    if request.situation.pieces.is_empty() {
        return Err(RecommendUnavailability::UnsupportedInput);
    }
    for piece in request
        .situation
        .pieces
        .iter()
        .copied()
        .chain(request.situation.hold)
    {
        if piece_from_external(piece).is_none() {
            return Err(RecommendUnavailability::UnsupportedInput);
        }
    }
    if !(0.0..=64.0).contains(&request.situation.mult) {
        return Err(RecommendUnavailability::UnsupportedInput);
    }
    if request.situation.beam_width == 0
        || request.situation.b2b < -1
        || request.situation.combo < -1
        || request.situation.pending < 0
        || request
            .situation
            .gmask
            .iter()
            .any(|row| row & !u32::from(ROW_MASK_10BIT) != 0)
    {
        return Err(RecommendUnavailability::UnsupportedInput);
    }
    Ok(())
}

fn completion_of(requested: usize, completed: usize) -> RecommendCompletion {
    if completed == 0 {
        RecommendCompletion::Empty
    } else if completed >= requested {
        RecommendCompletion::Complete
    } else {
        RecommendCompletion::Partial
    }
}

fn coach_step_mechanics(step: &CoachLineStep) -> RecommendMechanics {
    RecommendMechanics {
        rows_after: Some(step.rows.to_vec()),
        raw_attack: Some(step.attack),
        surge_delta: Some(step.surge_potential_delta),
        lines: Some(step.lines),
        b2b_after: Some(step.b2b),
        combo_after: Some(step.combo),
        b2b_before: Some(step.b2b_before),
        combo_before: Some(step.combo_before),
        is_surge_release: Some(step.is_surge_release),
    }
}

/// Normalize a fixed-continuation beam line (S2 / human-prior shape) into the
/// common candidate. `shaped` is the beam's selection-shaped total: raw
/// attack sent plus banked surge-potential change. `final_gmask` is `None`
/// when the producing beam does not track garbage state.
#[allow(clippy::too_many_arguments)]
pub fn normalize_coach_line(
    source: RecommendSourceId,
    config: &str,
    artifact: Option<String>,
    line: &CoachLineResult,
    requested_plies: usize,
    final_gmask: Option<Vec<u32>>,
    assumptions: RecommendAssumptions,
) -> RecommendCandidate {
    let path = line
        .steps
        .iter()
        .map(|s| RecommendPlacement {
            legacy_score: None,
            piece: s.mv.piece,
            rotation: s.mv.rotation,
            x: s.mv.x,
            y: s.mv.y,
            spin: s.mv.spin,
            hold_used: false,
        })
        .collect::<Vec<_>>();
    let completed = path.len();
    let mechanics = line
        .steps
        .iter()
        .map(coach_step_mechanics)
        .collect::<Vec<_>>();
    let raw_total: f64 = line.steps.iter().map(|s| s.attack).sum();
    let surge_total: f64 = line.steps.iter().map(|s| s.surge_potential_delta).sum();
    RecommendCandidate {
        source,
        config: config.to_owned(),
        artifact,
        path,
        completion: completion_of(requested_plies, completed),
        requested_plies,
        completed_plies: completed,
        mechanics,
        shaped_value: Some(line.attack),
        fallback_used: None,
        evidence: None,
        raw_total: Some(raw_total),
        surge_total: Some(surge_total),
        final_rows: Some(line.final_rows.to_vec()),
        final_gmask,
        scores: RecommendScores {
            selection_score: Some(line.selection_score),
            ..RecommendScores::default()
        },
        assumptions,
    }
}

/// One PV step with its reconstructed hold flag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativePathStep {
    pub placement: RecommendPlacement,
}

/// The (current, hold, queue) envelope a native PV is replayed against.
#[derive(Clone, Copy, Debug)]
pub struct NativeEnvelope<'a> {
    pub current: Piece,
    pub hold: Option<Piece>,
    pub queue: &'a [Piece],
}

/// Reconstruct per-step hold flags for a native search PV by replaying the
/// actual (current, hold, queue) transitions described in
/// `search_expand::enumerate_actions`: playing the current piece leaves hold
/// untouched; playing the held piece (or the queue head when hold is empty)
/// swaps the current piece into hold. A PV that cannot be produced from the
/// given envelope is an error, not a silent all-false path.
pub fn reconstruct_native_hold_flags(
    envelope: &NativeEnvelope<'_>,
    pv: &[Move],
) -> Result<Vec<bool>, RecommendUnavailability> {
    reconstruct_hold_flags(envelope, pv, None)
}

fn reconstruct_hold_flags(
    envelope: &NativeEnvelope<'_>,
    pv: &[Move],
    root_hold: Option<bool>,
) -> Result<Vec<bool>, RecommendUnavailability> {
    let mut cur = envelope.current;
    let mut held = envelope.hold;
    let mut rest = envelope.queue;
    let mut flags = Vec::with_capacity(pv.len());
    for (index, mv) in pv.iter().enumerate() {
        let piece = mv.piece();
        if piece == cur && !(index == 0 && root_hold == Some(true)) {
            flags.push(false);
        } else if held == Some(piece) && !(index == 0 && root_hold == Some(false)) {
            flags.push(true);
            held = Some(cur);
        } else if held.is_none()
            && rest.first().copied() == Some(piece)
            && !(index == 0 && root_hold == Some(false))
        {
            flags.push(true);
            held = Some(cur);
            rest = rest.get(1..).unwrap_or(&[]);
        } else {
            return Err(RecommendUnavailability::UnsupportedInput);
        }
        if index + 1 < pv.len() {
            cur = rest
                .first()
                .copied()
                .ok_or(RecommendUnavailability::NoLegalContinuation)?;
            rest = rest.get(1..).unwrap_or(&[]);
        }
    }
    Ok(flags)
}

/// Options for [`normalize_search_result`].
#[derive(Clone, Copy, Debug)]
pub struct NativeNormalizeOptions {
    pub requested_plies: usize,
    pub queue_extension_7bag: bool,
    pub garbage_multiplier: f64,
}

/// Normalize an existing native search result where the replay-analysis
/// interface consumes it. The best PV becomes the single candidate; the
/// remaining root scores stay supplemental diagnostics and never become
/// lines. Replay the selected path through the same lock transition as search;
/// its clear-event history omits non-clearing placements.
pub fn normalize_search_result(
    state: &GameState,
    full: &SearchResultFull,
    options: &NativeNormalizeOptions,
) -> Result<RecommendCandidate, RecommendUnavailability> {
    let best = &full.best;
    if best.pv.is_empty() {
        return Err(RecommendUnavailability::NoLegalContinuation);
    }
    let extended_queue = if options.queue_extension_7bag {
        crate::bag::extend_queue(&state.queue, state.current, state.hold)
    } else {
        state.queue.clone()
    };
    let envelope = NativeEnvelope {
        current: state.current,
        hold: state.hold,
        queue: &extended_queue,
    };
    let hold_flags = reconstruct_hold_flags(&envelope, &best.pv, Some(best.hold_used))?;
    let path = best
        .pv
        .iter()
        .zip(hold_flags.iter())
        .map(|(m, hold_used)| RecommendPlacement {
            legacy_score: None,
            piece: piece_to_external(m.piece()),
            rotation: m.rotation() as u8,
            x: m.x() as i8,
            y: m.y() as i8,
            spin: m.spin() as u8,
            hold_used: *hold_used,
        })
        .collect::<Vec<_>>();
    let completed = path.len();
    let root_scores = full
        .root_scores
        .iter()
        .map(|(m, score)| RecommendRootScore {
            piece: piece_to_external(m.piece()),
            rotation: m.rotation() as u8,
            x: m.x() as i8,
            y: m.y() as i8,
            spin: m.spin() as u8,
            score: *score,
        })
        .collect::<Vec<_>>();
    let mut board = state.board.clone();
    let mut chain = state.chain_state();
    let attack_config = crate::attack::AttackConfig {
        garbage_multiplier: options.garbage_multiplier as f32,
        ..crate::attack::AttackConfig::tetra_league()
    };
    let multiplier = f64::from(attack_config.garbage_multiplier);
    let mut mechanics = Vec::with_capacity(completed);
    for (index, mv) in best.pv.iter().enumerate() {
        let lock = board.lock(mv);
        let transition = chain.advance_lock(
            mv,
            &lock,
            hold_flags[index],
            GameState::spawn_envelope_blocked(&board),
            &attack_config,
        );
        let b2b_before = i32::from(chain.b2b) - 1;
        let b2b_after = i32::from(transition.chain.b2b) - 1;
        mechanics.push(RecommendMechanics {
            rows_after: Some(board.rows.to_vec()),
            raw_attack: Some(f64::from(transition.attack)),
            lines: Some(lock.lines_cleared),
            b2b_before: Some(b2b_before),
            b2b_after: Some(b2b_after),
            combo_before: Some(chain.combo as i32 - 1),
            combo_after: Some(transition.chain.combo as i32 - 1),
            is_surge_release: Some(
                transition
                    .clear_event
                    .is_some_and(|event| event.is_surge_release),
            ),
            surge_delta: Some(
                (crate::attack::surge_potential(b2b_after, multiplier)
                    - crate::attack::surge_potential(b2b_before, multiplier))
                    as f64,
            ),
        });
        chain = transition.chain;
    }
    let raw_total: Option<f64> = mechanics.iter().map(|step| step.raw_attack).sum();
    let surge_total: Option<f64> = mechanics.iter().map(|step| step.surge_delta).sum();
    Ok(RecommendCandidate {
        source: RecommendSourceId::NativeSearch,
        config: "full-search".to_owned(),
        artifact: None,
        path,
        completion: completion_of(options.requested_plies, completed),
        requested_plies: options.requested_plies,
        completed_plies: completed,
        mechanics,
        shaped_value: raw_total.zip(surge_total).map(|(raw, surge)| raw + surge),
        fallback_used: Some(full.fallback_used),
        evidence: None,
        raw_total,
        surge_total,
        final_rows: Some(board.rows.to_vec()),
        final_gmask: None,
        scores: RecommendScores {
            native_composite: Some(best.score),
            root_scores,
            ..RecommendScores::default()
        },
        assumptions: RecommendAssumptions {
            garbage_model: Some(RecommendGarbageModel::PendingClearHeuristic),
            hold_supported: true,
            queue_extension_7bag: options.queue_extension_7bag,
            seed_b2b: i32::from(state.b2b) - 1,
            seed_combo: state.combo as i32 - 1,
            seed_pending: i32::from(state.pending_garbage),
            garbage_mult: multiplier,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::coach_beam::CoachLineMove;

    fn empty_situation(pieces: Vec<u8>) -> RecommendSituation {
        RecommendSituation {
            board_rows: vec![0u16; BOARD_ROWS],
            gmask: vec![0u32; BOARD_ROWS],
            pieces,
            hold: None,
            b2b: 0,
            combo: 0,
            pending: 0,
            mult: 1.0,
            beam_width: 300,
        }
    }

    fn fixed_request(situation: RecommendSituation) -> RecommendRequest {
        RecommendRequest {
            version: RECOMMEND_CONTRACT_VERSION,
            source: RecommendSourceId::S2Fixed,
            config: "exact".to_owned(),
            input: RecommendInput::FixedSequence {
                extend_beyond_supplied: false,
                hold_supported: false,
            },
            situation,
            conditioning: RecommendConditioning::default(),
            options: None,
        }
    }

    #[test]
    fn valid_fixed_sequence_request_passes_validation() {
        let request = fixed_request(empty_situation(vec![0, 1, 2]));
        assert_eq!(validate_request(&request), Ok(()));
    }

    #[test]
    fn rank_or_history_conditioning_is_rejected_not_dropped() {
        let mut request = fixed_request(empty_situation(vec![0, 1, 2]));
        request.conditioning.rank = Some("x".to_owned());
        assert_eq!(
            validate_request(&request),
            Err(RecommendUnavailability::UnsupportedConditioning)
        );
        let mut history = fixed_request(empty_situation(vec![0, 1, 2]));
        history.conditioning.history = Some("owner".to_owned());
        assert_eq!(
            validate_request(&history),
            Err(RecommendUnavailability::UnsupportedConditioning)
        );
    }

    #[test]
    fn unknown_piece_ids_and_bad_rows_are_rejected() {
        let mut bad_piece = fixed_request(empty_situation(vec![0, 9]));
        assert_eq!(
            validate_request(&bad_piece),
            Err(RecommendUnavailability::UnsupportedInput)
        );
        bad_piece.situation.pieces = vec![0];
        bad_piece.situation.hold = Some(7);
        assert_eq!(
            validate_request(&bad_piece),
            Err(RecommendUnavailability::UnsupportedInput)
        );
        let mut bad_rows = fixed_request(empty_situation(vec![0]));
        bad_rows.situation.board_rows[3] = 1u16 << 10;
        assert_eq!(
            validate_request(&bad_rows),
            Err(RecommendUnavailability::UnsupportedInput)
        );
        let empty = fixed_request(empty_situation(vec![]));
        assert_eq!(
            validate_request(&empty),
            Err(RecommendUnavailability::UnsupportedInput)
        );
    }

    fn sample_line() -> CoachLineResult {
        CoachLineResult {
            attack: 9.0,
            selection_score: 9.0,
            steps: vec![
                crate::coach_beam::CoachLineStep {
                    rows: [0u16; BOARD_ROWS],
                    attack: 4.0,
                    lines: 2,
                    b2b: 0,
                    combo: 0,
                    spin: 0,
                    b2b_before: 0,
                    combo_before: 0,
                    is_surge_release: false,
                    surge_potential_delta: 0.0,
                    mv: CoachLineMove {
                        piece: 1,
                        rotation: 0,
                        x: 4,
                        y: 0,
                        spin: 0,
                    },
                },
                crate::coach_beam::CoachLineStep {
                    rows: [0u16; BOARD_ROWS],
                    attack: 2.0,
                    lines: 1,
                    b2b: 0,
                    combo: 1,
                    spin: 0,
                    b2b_before: 0,
                    combo_before: 0,
                    is_surge_release: false,
                    surge_potential_delta: 3.0,
                    mv: CoachLineMove {
                        piece: 2,
                        rotation: 1,
                        x: 5,
                        y: 1,
                        spin: 0,
                    },
                },
            ],
            final_rows: [0u16; BOARD_ROWS],
        }
    }

    fn fixed_assumptions() -> RecommendAssumptions {
        RecommendAssumptions {
            garbage_model: Some(RecommendGarbageModel::OriginMask),
            hold_supported: false,
            queue_extension_7bag: false,
            seed_b2b: 0,
            seed_combo: 0,
            seed_pending: 0,
            garbage_mult: 1.0,
        }
    }

    #[test]
    fn coach_line_keeps_score_identity_and_marks_partial_completion() {
        let candidate = normalize_coach_line(
            RecommendSourceId::S2Fixed,
            "exact",
            None,
            &sample_line(),
            5,
            None,
            fixed_assumptions(),
        );
        assert_eq!(candidate.completion, RecommendCompletion::Partial);
        assert_eq!(candidate.completed_plies, 2);
        assert_eq!(candidate.raw_total, Some(6.0));
        assert_eq!(candidate.surge_total, Some(3.0));
        assert_eq!(candidate.shaped_value, Some(9.0));
        assert_eq!(
            candidate.mechanics[0].raw_attack,
            Some(4.0),
            "per-step attack stays raw"
        );
        assert!(
            candidate.final_gmask.is_none(),
            "absent gmask must not become a zero array"
        );
        assert!(candidate.final_rows.is_some());
    }

    #[test]
    fn coach_line_marks_complete_when_sequence_is_fully_covered() {
        let candidate = normalize_coach_line(
            RecommendSourceId::S2Fixed,
            "exact",
            None,
            &sample_line(),
            2,
            Some(vec![0u32; BOARD_ROWS]),
            fixed_assumptions(),
        );
        assert_eq!(candidate.completion, RecommendCompletion::Complete);
        assert!(candidate.final_gmask.is_some());
    }

    #[test]
    fn native_hold_flags_follow_actual_transitions() {
        use crate::header::Rotation;
        // Envelope: current T, hold I, queue [O, ...]. PV plays I (held),
        // then O (queue head after the swap).
        let board = Board::new();
        let state = GameState {
            board,
            current: Piece::T,
            hold: Some(Piece::I),
            queue: vec![Piece::O, Piece::S],
            b2b: 0,
            combo: 0,
            pending_garbage: 0,
            lines_total: 0,
            bag_number: 0,
            pieces_into_bag: 0,
            coaching: crate::state::CoachingState::default(),
        };
        let held_move = Move::new(Piece::I, Rotation::North, 4, 0, false);
        let next_move = Move::new(Piece::O, Rotation::North, 4, 0, false);
        let envelope = NativeEnvelope {
            current: state.current,
            hold: state.hold,
            queue: &state.queue,
        };
        let flags = reconstruct_native_hold_flags(&envelope, &[held_move, next_move])
            .expect("consistent PV must reconstruct");
        assert_eq!(flags, vec![true, false]);
    }

    #[test]
    fn native_hold_flags_reject_unproducible_paths() {
        let board = Board::new();
        let state = GameState {
            board,
            current: Piece::T,
            hold: None,
            queue: vec![Piece::O],
            b2b: 0,
            combo: 0,
            pending_garbage: 0,
            lines_total: 0,
            bag_number: 0,
            pieces_into_bag: 0,
            coaching: crate::state::CoachingState::default(),
        };
        // Z is neither current nor reachable through hold/queue.
        let impossible = Move::new(Piece::Z, crate::header::Rotation::North, 4, 0, false);
        let envelope = NativeEnvelope {
            current: state.current,
            hold: state.hold,
            queue: &state.queue,
        };
        assert_eq!(
            reconstruct_native_hold_flags(&envelope, &[impossible]),
            Err(RecommendUnavailability::UnsupportedInput)
        );
    }

    #[test]
    fn normalize_real_search_result_keeps_diagnostics_supplemental() {
        use crate::eval::EvalWeights;
        use crate::search::{search, SearchConfig, SearchRequest};
        let config = SearchConfig {
            beam_width: 20,
            depth: 3,
            extend_queue_7bag: false,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();
        let state = GameState {
            board: Board::new(),
            current: Piece::T,
            hold: None,
            queue: vec![Piece::I, Piece::O, Piece::S, Piece::Z],
            b2b: 0,
            combo: 0,
            pending_garbage: 0,
            lines_total: 0,
            bag_number: 0,
            pieces_into_bag: 0,
            coaching: crate::state::CoachingState::default(),
        };
        let full = search(
            &state,
            &SearchRequest {
                config: &config,
                weights: &weights,
                runtime: None,
                forced_root_move: None,
            },
        )
        .expect("empty-board search must produce a result");
        let candidate = normalize_search_result(
            &state,
            &full,
            &NativeNormalizeOptions {
                requested_plies: 3,
                queue_extension_7bag: false,
                garbage_multiplier: 1.0,
            },
        )
        .expect("PV must normalize");
        assert!(!candidate.path.is_empty());
        assert!(candidate.path.iter().all(|p| p.piece <= 6));
        assert_ne!(candidate.completion, RecommendCompletion::Empty);
        assert_eq!(candidate.mechanics.len(), candidate.path.len());
        assert!(candidate
            .mechanics
            .iter()
            .all(|step| step.rows_after.is_some()));
        assert_eq!(
            candidate.raw_total,
            Some(0.0),
            "the replayed opening has no clears"
        );
        assert_eq!(candidate.scores.native_composite, Some(full.best.score));
        assert!(
            !candidate.scores.root_scores.is_empty(),
            "root scores travel as supplemental diagnostics"
        );
        assert!(
            candidate.scores.root_scores.len() != candidate.completed_plies
                || candidate.completed_plies == 1,
            "root scores must not be mistaken for candidate lines"
        );
        assert!(candidate.assumptions.hold_supported);
        assert!(!candidate.assumptions.queue_extension_7bag);
    }
}
