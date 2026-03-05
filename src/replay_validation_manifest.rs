use crate::replay_validation::ReplaySample;

pub fn parse_replay_samples_from_players_manifest(
    manifest: &str,
) -> Result<Vec<ReplaySample>, String> {
    let mut in_players = false;
    let mut in_player = false;
    let mut in_replay_ids = false;

    let mut current_rank: Option<String> = None;
    let mut current_replay_ids: Vec<String> = Vec::new();
    let mut samples: Vec<ReplaySample> = Vec::new();

    for line in manifest.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("\"players\": [") {
            in_players = true;
            continue;
        }
        if !in_players {
            continue;
        }

        if !in_player && (trimmed == "]," || trimmed == "]") {
            break;
        }

        if trimmed == "{" {
            in_player = true;
            in_replay_ids = false;
            current_rank = None;
            current_replay_ids.clear();
            continue;
        }

        if !in_player {
            continue;
        }

        if let Some(rank) = parse_string_field(trimmed, "rank") {
            current_rank = Some(rank);
            continue;
        }

        if trimmed.starts_with("\"replay_ids\": [") {
            in_replay_ids = true;
            continue;
        }

        if in_replay_ids {
            if trimmed == "]," || trimmed == "]" {
                in_replay_ids = false;
            } else if let Some(replay_id) = parse_array_string_item(trimmed) {
                current_replay_ids.push(replay_id);
            }
            continue;
        }

        if trimmed == "}," || trimmed == "}" {
            if let Some(rank) = current_rank.as_ref() {
                for replay_id in &current_replay_ids {
                    samples.push(ReplaySample {
                        replay_id: replay_id.clone(),
                        rank: rank.clone(),
                    });
                }
            }

            in_player = false;
            in_replay_ids = false;
            current_rank = None;
            current_replay_ids.clear();
        }
    }

    if samples.is_empty() {
        return Err("no replay samples parsed from players manifest".to_string());
    }

    samples.sort_by(|a, b| a.replay_id.cmp(&b.replay_id).then(a.rank.cmp(&b.rank)));
    Ok(samples)
}

fn parse_string_field(line: &str, key: &str) -> Option<String> {
    let prefix = format!("\"{}\": \"", key);
    let stripped = line.strip_prefix(&prefix)?;
    let end_idx = stripped.find('"')?;
    Some(stripped[..end_idx].to_string())
}

fn parse_array_string_item(line: &str) -> Option<String> {
    let stripped = line.strip_prefix('"')?;
    let end_idx = stripped.find('"')?;
    Some(stripped[..end_idx].to_string())
}
