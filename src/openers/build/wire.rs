//! The planner's wire contract: the JSON input, the plan and its parts, and
//! the request errors.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::openers::catalog::{CatalogError, OpenerTreeNode};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenerBuildInput {
    pub(super) tree: Vec<OpenerTreeNode>,
    #[serde(default)]
    pub(super) mirrored: bool,
    /// Shapes of one version: nodes whose paths perform the same clears in
    /// the same order. The build takes the shape the queue fits best.
    pub(super) targets: Vec<u32>,
    #[serde(default)]
    pub(super) start_node: Option<u32>,
    #[serde(default)]
    pub(super) queue: String,
    /// Plan the default order instead of `queue` and `hold`.
    #[serde(default)]
    pub(super) default_queue: bool,
    #[serde(default)]
    pub(super) hold: Option<String>,
    #[serde(default = "hold_enabled_default")]
    pub(super) hold_enabled: bool,
}

const fn hold_enabled_default() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerBuildPlan {
    pub status: BuildStatus,
    pub mirrored: bool,
    pub start_node: Option<u32>,
    pub target_node: u32,
    /// The queue planned: the request's, or the default order.
    pub queue: String,
    /// Floor-up rows; `X` marks a target cell the source leaves unlettered.
    pub start_board: Vec<String>,
    pub target_board: Vec<String>,
    pub stages: Vec<BuildStage>,
    /// The clears the opener is built for, in order.
    pub clears: Vec<PlannedClear>,
    /// The playable order: empty unless the status is complete or
    /// needsMorePieces. A dead end's pieces are in `stuck.placed`.
    pub steps: Vec<BuildStep>,
    /// Attack the steps send under the S2/Tetra League rules.
    pub attack: f32,
    /// The hold the build starts with.
    pub start_hold: Option<String>,
    /// The hold after the last step; `start_hold` when there are no steps.
    pub end_hold: Option<String>,
    /// The last queue piece goes to the empty hold after the final step;
    /// `end_hold` does not include it.
    pub trailing_hold: bool,
    pub queue_used: usize,
    pub remaining: BuildRemaining,
    pub reasons: Vec<BuildReason>,
    pub stuck: Option<BuildStuck>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BuildStatus {
    Complete,
    NeedsMorePieces,
    Impossible,
    /// The search budget ran out before an answer.
    Undetermined,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStage {
    pub node_id: u32,
    pub pieces: u32,
}

/// One clear the catalog lists on the path, as the build must perform it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedClear {
    pub node_id: u32,
    /// The catalog's name for it; none when the source does not say how
    /// these rows clear.
    pub label: Option<String>,
    pub lines: u8,
    /// The piece a spin clear needs.
    pub piece: Option<String>,
    /// The least spin that counts.
    pub spin: SpinName,
    /// Whether the plan holds the build to this clear. False when the label
    /// cannot be checked against the rows (several clears share one page).
    pub enforced: bool,
    /// No order performs the clear as labelled, so `piece` and `spin` are the
    /// most the diagram allows.
    pub relaxed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepClear {
    pub label: String,
    pub lines: u8,
    pub spin: SpinName,
    pub attack: f32,
    /// Back-to-back count after the clear; -1 when no chain is running.
    pub b2b: i32,
    pub combo: i32,
    pub perfect_clear: bool,
    /// The catalogued clear this performs.
    pub catalogued: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep {
    pub piece: String,
    pub hold_action: HoldActionName,
    pub queue_index: Option<usize>,
    pub hold_after: Option<String>,
    /// Floor-up cells on the board before this lock.
    pub cells: Vec<[u8; 2]>,
    pub spin: SpinName,
    /// Triangle key names from the hold press to the hard drop.
    pub keys: Vec<&'static str>,
    /// The piece's cells at spawn and after every movement key.
    pub frames: Vec<Vec<[i16; 2]>>,
    /// Rows this lock clears, on the board before the clear.
    pub cleared_rows: Vec<u8>,
    /// Floor-up rows after the lock and its clears.
    pub board: Vec<String>,
    /// The tree node this step finishes, if any.
    pub stage: Option<u32>,
    pub clear: Option<StepClear>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HoldActionName {
    None,
    Swap,
    Stash,
    /// The held piece is played by holding with the piece after the queue.
    SwapNext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SpinName {
    None,
    Mini,
    Full,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRemaining {
    /// Catalogued pieces not placed yet, lowest first.
    pub pieces: Vec<String>,
    /// Pieces still needed for cells the source leaves unlettered.
    pub outline_pieces: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BuildReason {
    /// A node's diagram does not extend its parent's, or its clears disagree
    /// with its rows.
    SourceMismatch {
        node_id: u32,
    },
    /// The path clears more rows before this node than one plan tracks;
    /// starting from a later node shortens it.
    TooLong {
        node_id: u32,
    },
    /// The source shows more of the board as an outline than one plan fills
    /// (it names no pieces for these cells).
    OutlineOnly {
        cells: u32,
    },
    /// No order of any pieces builds the diagram under SRS+.
    Unbuildable,
    /// The queue holds enough pieces but too few of these letters.
    MissingPieces {
        pieces: Vec<MissingPiece>,
    },
    /// The pieces are there, but their order leaves no legal plan.
    OrderConflict,
    /// The queue builds the shape, but not with this catalogued clear.
    ClearsLost {
        node_id: u32,
        label: String,
    },
    /// No order of any pieces builds the shape with this catalogued clear,
    /// though the shape alone can be built.
    ClearUnachievable {
        node_id: u32,
        label: String,
    },
    SearchLimit,
    /// The plan's key paths could not be replayed; an engine defect.
    PathReplayFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingPiece {
    pub piece: String,
    pub needed: u32,
    pub available: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStuck {
    /// Pieces placed before the dead end (`placed.len()`).
    pub after_step: usize,
    /// The pieces placed before the dead end, in the order the search
    /// placed them.
    pub placed: Vec<PlacedPiece>,
    pub board: Vec<String>,
    pub hold: Option<String>,
    pub pieces: Vec<StuckPiece>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedPiece {
    pub piece: String,
    /// Floor-up cells on the board before this lock, as `BuildStep::cells`.
    pub cells: Vec<[u8; 2]>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StuckPiece {
    pub piece: String,
    pub source: PieceSource,
    /// False for a piece the rest of the build never uses.
    pub in_build: bool,
    /// For unlettered outline cells: whether the piece fits them anywhere.
    pub outline_fit: Option<bool>,
    pub slots: Vec<StuckSlot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PieceSource {
    Current,
    Hold,
    Next,
    /// Any-order analysis: the piece stands for any copy of its letter.
    Any,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StuckSlot {
    pub cells: Vec<[u8; 2]>,
    pub verdict: SlotVerdict,
    /// Unplaced pieces under a floating spot.
    pub waits_for: Vec<Vec<[u8; 2]>>,
    /// Placed pieces whose removal alone would open a sealed spot.
    pub blocked_by: Vec<Vec<[u8; 2]>>,
    /// The catalogued clear the verdict concerns.
    pub clear: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SlotVerdict {
    /// Lockable now; the dead end comes later.
    Placeable,
    NeedsSupport,
    Blocked,
    /// The spin piece would lock before the rest of its rows is filled.
    TooEarly,
    /// No path from spawn gives it the spin its clear needs.
    NoSpin,
    /// It would clear catalogued rows the wrong way.
    BreaksClear,
    /// It would clear before an earlier catalogued clear.
    OutOfOrder,
}

#[derive(Debug)]
pub enum OpenerBuildError {
    InvalidTree(CatalogError),
    InvalidQueue,
    UnknownNode(u32),
    StartNotAncestor,
    NothingToBuild,
    /// The request names no target.
    NoTargets,
    /// The targets perform different clears.
    MixedVersions,
}

impl OpenerBuildError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidTree(_) => "invalidTree",
            Self::InvalidQueue => "invalidQueue",
            Self::UnknownNode(_) => "unknownNode",
            Self::StartNotAncestor => "startNotAncestor",
            Self::NothingToBuild => "nothingToBuild",
            Self::NoTargets => "noTargets",
            Self::MixedVersions => "mixedVersions",
        }
    }
}

impl fmt::Display for OpenerBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTree(error) => write!(formatter, "invalid opener tree: {error}"),
            Self::InvalidQueue => formatter.write_str(
                "queue and hold must use the letters IJLOSTZ, with at most 70 queue pieces",
            ),
            Self::UnknownNode(id) => write!(formatter, "tree has no node {id}"),
            Self::StartNotAncestor => {
                formatter.write_str("start node is not an ancestor of the target")
            }
            Self::NothingToBuild => formatter.write_str("start and target are the same node"),
            Self::NoTargets => formatter.write_str("no target node"),
            Self::MixedVersions => formatter.write_str("targets perform different clears"),
        }
    }
}

impl std::error::Error for OpenerBuildError {}
