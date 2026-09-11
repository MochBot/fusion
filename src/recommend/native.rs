use super::*;
use crate::board::Board;
use crate::eval::EvalWeights;
use crate::search::{search, SearchConfig, SearchRequest};

pub fn recommend_json(request_json: &str) -> String {
    let outcome = match serde_json::from_str::<RecommendRequest>(request_json) {
        Ok(request) => recommend_native(&request),
        Err(_) => RecommendOutcome::Unavailable {
            reason: RecommendUnavailability::UnsupportedInput,
        },
    };
    match serde_json::to_string(&outcome) {
        Ok(json) => json,
        Err(error) => {
            serde_json::json!({"status": "failed", "error": error.to_string()}).to_string()
        }
    }
}

pub fn recommend_native(request: &RecommendRequest) -> RecommendOutcome {
    if let Err(reason) = validate_request(request) {
        return RecommendOutcome::Unavailable { reason };
    }
    let RecommendInput::FullPosition {
        hold_supported: true,
        queue_extension_7bag,
    } = request.input
    else {
        return RecommendOutcome::Unavailable {
            reason: RecommendUnavailability::UnsupportedInput,
        };
    };
    if request.source != RecommendSourceId::NativeSearch
        || request.config != "full-search"
        || request.situation.gmask.iter().any(|row| *row != 0)
        || request.options.as_ref().is_some_and(|options| {
            [
                options.terminal_lambda,
                options.dig_combo_w,
                options.dig_attack_w,
                options.spin_w,
                options.height_w,
                options.height_grace,
                options.b2b_floor,
            ]
            .iter()
            .any(Option::is_some)
        })
    {
        return RecommendOutcome::Unavailable {
            reason: RecommendUnavailability::UnsupportedInput,
        };
    }
    let situation = &request.situation;
    let Some(current) = situation
        .pieces
        .first()
        .and_then(|piece| piece_from_external(*piece))
    else {
        return RecommendOutcome::Unavailable {
            reason: RecommendUnavailability::UnsupportedInput,
        };
    };
    let mut board = Board::new();
    for (y, row) in situation.board_rows.iter().copied().enumerate() {
        board.rows[y] = row;
        for x in 0..10 {
            if row & (1 << x) != 0 {
                board.cols[x] |= 1 << y;
            }
        }
    }
    let queue = situation
        .pieces
        .iter()
        .skip(1)
        .filter_map(|piece| piece_from_external(*piece))
        .collect();
    let mut state = GameState::new(board, current, queue);
    state.hold = situation.hold.and_then(piece_from_external);
    let (Some(b2b), Some(combo), Ok(pending)) = (
        situation
            .b2b
            .checked_add(1)
            .and_then(|value| u8::try_from(value).ok()),
        situation
            .combo
            .checked_add(1)
            .and_then(|value| u32::try_from(value).ok()),
        u8::try_from(situation.pending),
    ) else {
        return RecommendOutcome::Unavailable {
            reason: RecommendUnavailability::UnsupportedInput,
        };
    };
    state.b2b = b2b;
    state.combo = combo;
    state.pending_garbage = pending;
    let mut config = SearchConfig {
        beam_width: situation.beam_width as usize,
        depth: situation.pieces.len(),
        extend_queue_7bag: queue_extension_7bag,
        ..SearchConfig::default()
    };
    config.attack_config.garbage_multiplier = situation.mult as f32;
    let weights = EvalWeights::default();
    let Some(result) = search(
        &state,
        &SearchRequest {
            config: &config,
            weights: &weights,
            runtime: None,
            forced_root_move: None,
        },
    ) else {
        return RecommendOutcome::Unavailable {
            reason: RecommendUnavailability::NoLegalContinuation,
        };
    };
    match normalize_search_result(
        &state,
        &result,
        &NativeNormalizeOptions {
            requested_plies: situation.pieces.len(),
            queue_extension_7bag,
            garbage_multiplier: f64::from(config.attack_config.garbage_multiplier),
        },
    ) {
        Ok(candidate) => RecommendOutcome::Ready {
            candidate: Box::new(candidate),
        },
        Err(reason) => RecommendOutcome::Unavailable { reason },
    }
}
