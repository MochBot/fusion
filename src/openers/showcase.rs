//! Dealt-piece inputs kept on the round input contract. The guide does not
//! consume them; they remain so callers keep one input shape.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowcaseDealtInput {
    pub spawn_piece: ExternalPiece,
    pub hold: Option<ExternalPiece>,
    pub queue_head: Option<ExternalPiece>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct ExternalPiece(pub(crate) u8);

impl TryFrom<u8> for ExternalPiece {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value <= 6 {
            Ok(Self(value))
        } else {
            Err("external piece IDs must be in 0..=6")
        }
    }
}

impl From<ExternalPiece> for u8 {
    fn from(piece: ExternalPiece) -> Self {
        piece.0
    }
}
