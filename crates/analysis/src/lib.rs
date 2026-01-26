//! Fusion analysis crate - misdrop detection and coaching moments.

mod misdrop;
mod moments;
mod pipeline;

pub use misdrop::{detect_misdrop, Misdrop, MisdropSeverity};
pub use moments::{detect_missed_tspin, generate_moments, GameStats, Moment, MomentType};
pub use pipeline::{analyze_replay, AnalysisResult, ReplayFrame};
