use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SkillBucket {
    B,
    BPlus,
    AMinus,
    A,
    APlus,
    SMinus,
    S,
    SPlus,
    SS,
    U,
}

impl SkillBucket {
    pub const ORDERED: [SkillBucket; 10] = [
        SkillBucket::B,
        SkillBucket::BPlus,
        SkillBucket::AMinus,
        SkillBucket::A,
        SkillBucket::APlus,
        SkillBucket::SMinus,
        SkillBucket::S,
        SkillBucket::SPlus,
        SkillBucket::SS,
        SkillBucket::U,
    ];

    pub fn as_rank_str(self) -> &'static str {
        match self {
            SkillBucket::B => "b",
            SkillBucket::BPlus => "b+",
            SkillBucket::AMinus => "a-",
            SkillBucket::A => "a",
            SkillBucket::APlus => "a+",
            SkillBucket::SMinus => "s-",
            SkillBucket::S => "s",
            SkillBucket::SPlus => "s+",
            SkillBucket::SS => "ss",
            SkillBucket::U => "u",
        }
    }

    pub fn from_rank_str(rank: &str) -> Option<Self> {
        match rank {
            "b" => Some(SkillBucket::B),
            "b+" => Some(SkillBucket::BPlus),
            "a-" => Some(SkillBucket::AMinus),
            "a" => Some(SkillBucket::A),
            "a+" => Some(SkillBucket::APlus),
            "s-" => Some(SkillBucket::SMinus),
            "s" => Some(SkillBucket::S),
            "s+" => Some(SkillBucket::SPlus),
            "ss" => Some(SkillBucket::SS),
            "u" => Some(SkillBucket::U),
            _ => None,
        }
    }
}

impl fmt::Display for SkillBucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_rank_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BucketThresholds {
    pub none_max: f32,
    pub inaccuracy_max: f32,
    pub mistake_max: f32,
}

impl BucketThresholds {
    pub fn validate(self) -> Result<(), String> {
        if !self.none_max.is_finite()
            || !self.inaccuracy_max.is_finite()
            || !self.mistake_max.is_finite()
        {
            return Err("thresholds must be finite".to_string());
        }
        if self.none_max < 0.0 {
            return Err("none_max must be >= 0".to_string());
        }
        if self.inaccuracy_max < self.none_max {
            return Err("inaccuracy_max must be >= none_max".to_string());
        }
        if self.mistake_max < self.inaccuracy_max {
            return Err("mistake_max must be >= inaccuracy_max".to_string());
        }
        Ok(())
    }
}

pub const CALIBRATION_VERSION_V1: u32 = 1;

pub fn default_eval_thresholds() -> BucketThresholds {
    BucketThresholds {
        none_max: 0.5,
        inaccuracy_max: 1.5,
        mistake_max: 3.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BucketCalibrationRow {
    pub bucket: SkillBucket,
    pub sample_count: u32,
    pub avg_tr: f64,
    pub thresholds: BucketThresholds,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationProfile {
    pub version: u32,
    pub source_fingerprint: u64,
    pub rows: Vec<BucketCalibrationRow>,
}

impl CalibrationProfile {
    pub fn thresholds_for(&self, bucket: SkillBucket) -> Option<BucketThresholds> {
        self.rows
            .iter()
            .find(|row| row.bucket == bucket)
            .map(|row| row.thresholds)
    }

    pub fn to_artifact_string(&self) -> String {
        let mut out = String::new();
        out.push_str("format=skill_bucket_calibration\n");
        out.push_str(&format!("version={}\n", self.version));
        out.push_str(&format!("source_fingerprint={}\n", self.source_fingerprint));
        for bucket in SkillBucket::ORDERED {
            if let Some(row) = self.rows.iter().find(|r| r.bucket == bucket) {
                out.push_str(&format!(
                    "bucket={},count={},avg_tr={:.6},none_max={:.6},inaccuracy_max={:.6},mistake_max={:.6}\n",
                    row.bucket.as_rank_str(),
                    row.sample_count,
                    row.avg_tr,
                    row.thresholds.none_max,
                    row.thresholds.inaccuracy_max,
                    row.thresholds.mistake_max
                ));
            }
        }
        out
    }

    pub fn from_artifact_str(input: &str) -> Result<Self, String> {
        let mut format_ok = false;
        let mut version = None;
        let mut source_fingerprint = None;
        let mut rows = Vec::new();

        for line in input.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(value) = trimmed.strip_prefix("format=") {
                format_ok = value == "skill_bucket_calibration";
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("version=") {
                version = Some(value.parse::<u32>().map_err(|e| e.to_string())?);
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("source_fingerprint=") {
                source_fingerprint = Some(value.parse::<u64>().map_err(|e| e.to_string())?);
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("bucket=") {
                let mut bucket = None;
                let mut sample_count = None;
                let mut avg_tr = None;
                let mut none_max = None;
                let mut inaccuracy_max = None;
                let mut mistake_max = None;

                let mut parts = value.split(',');
                if let Some(name) = parts.next() {
                    bucket = SkillBucket::from_rank_str(name);
                }
                for part in parts {
                    if let Some(v) = part.strip_prefix("count=") {
                        sample_count = Some(v.parse::<u32>().map_err(|e| e.to_string())?);
                    } else if let Some(v) = part.strip_prefix("avg_tr=") {
                        avg_tr = Some(v.parse::<f64>().map_err(|e| e.to_string())?);
                    } else if let Some(v) = part.strip_prefix("none_max=") {
                        none_max = Some(v.parse::<f32>().map_err(|e| e.to_string())?);
                    } else if let Some(v) = part.strip_prefix("inaccuracy_max=") {
                        inaccuracy_max = Some(v.parse::<f32>().map_err(|e| e.to_string())?);
                    } else if let Some(v) = part.strip_prefix("mistake_max=") {
                        mistake_max = Some(v.parse::<f32>().map_err(|e| e.to_string())?);
                    }
                }

                let thresholds = BucketThresholds {
                    none_max: none_max.ok_or_else(|| "missing none_max".to_string())?,
                    inaccuracy_max: inaccuracy_max
                        .ok_or_else(|| "missing inaccuracy_max".to_string())?,
                    mistake_max: mistake_max.ok_or_else(|| "missing mistake_max".to_string())?,
                };
                thresholds.validate()?;

                rows.push(BucketCalibrationRow {
                    bucket: bucket.ok_or_else(|| "invalid bucket".to_string())?,
                    sample_count: sample_count.ok_or_else(|| "missing count".to_string())?,
                    avg_tr: avg_tr.ok_or_else(|| "missing avg_tr".to_string())?,
                    thresholds,
                });
            }
        }

        if !format_ok {
            return Err("invalid calibration artifact format".to_string());
        }

        let version = version.ok_or_else(|| "missing version".to_string())?;
        let source_fingerprint =
            source_fingerprint.ok_or_else(|| "missing source_fingerprint".to_string())?;

        rows.sort_by_key(|row| row.bucket);
        Ok(Self {
            version,
            source_fingerprint,
            rows,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerBucketSample {
    pub bucket: SkillBucket,
    pub tr: f64,
}

pub fn parse_players_manifest_samples(manifest: &str) -> Result<Vec<PlayerBucketSample>, String> {
    let mut in_players = false;
    let mut in_player = false;
    let mut rank = None;
    let mut tr = None;
    let mut qualified = false;
    let mut samples = Vec::new();

    for line in manifest.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("\"players\": [") {
            in_players = true;
            continue;
        }
        if !in_players {
            continue;
        }

        if trimmed == "{" {
            in_player = true;
            rank = None;
            tr = None;
            qualified = false;
            continue;
        }

        if !in_player && (trimmed == "]," || trimmed == "]") {
            break;
        }

        if !in_player {
            continue;
        }

        if let Some(value) = parse_string_field(trimmed, "rank") {
            rank = SkillBucket::from_rank_str(&value);
            continue;
        }
        if let Some(value) = parse_f64_field(trimmed, "tr") {
            tr = Some(value);
            continue;
        }
        if let Some(value) = parse_bool_field(trimmed, "qualified") {
            qualified = value;
            continue;
        }

        if trimmed == "}," || trimmed == "}" {
            if qualified {
                if let (Some(bucket), Some(tr_value)) = (rank, tr) {
                    samples.push(PlayerBucketSample {
                        bucket,
                        tr: tr_value,
                    });
                }
            }
            in_player = false;
        }
    }

    if samples.is_empty() {
        return Err("no qualified samples parsed from players manifest".to_string());
    }

    Ok(samples)
}

pub fn generate_profile_from_samples(
    version: u32,
    samples: &[PlayerBucketSample],
) -> CalibrationProfile {
    let defaults = default_eval_thresholds();

    let mut rows = Vec::with_capacity(SkillBucket::ORDERED.len());
    for bucket in SkillBucket::ORDERED {
        let bucket_samples: Vec<f64> = samples
            .iter()
            .filter(|sample| sample.bucket == bucket)
            .map(|sample| sample.tr)
            .collect();
        let sample_count = bucket_samples.len() as u32;
        let avg_tr = if bucket_samples.is_empty() {
            0.0
        } else {
            bucket_samples.iter().sum::<f64>() / bucket_samples.len() as f64
        };

        let strength_scale = bucket_strength_scale(bucket);
        let thresholds = BucketThresholds {
            none_max: defaults.none_max * strength_scale,
            inaccuracy_max: defaults.inaccuracy_max * strength_scale,
            mistake_max: defaults.mistake_max * strength_scale,
        };

        rows.push(BucketCalibrationRow {
            bucket,
            sample_count,
            avg_tr,
            thresholds,
        });
    }

    CalibrationProfile {
        version,
        source_fingerprint: fingerprint_samples(samples),
        rows,
    }
}

pub fn generate_profile_from_players_manifest(
    version: u32,
    manifest: &str,
) -> Result<CalibrationProfile, String> {
    let samples = parse_players_manifest_samples(manifest)?;
    Ok(generate_profile_from_samples(version, &samples))
}

fn bucket_strength_scale(bucket: SkillBucket) -> f32 {
    match bucket {
        SkillBucket::B => 1.20,
        SkillBucket::BPlus => 1.16,
        SkillBucket::AMinus => 1.12,
        SkillBucket::A => 1.08,
        SkillBucket::APlus => 1.04,
        SkillBucket::SMinus => 1.00,
        SkillBucket::S => 0.96,
        SkillBucket::SPlus => 0.92,
        SkillBucket::SS => 0.88,
        SkillBucket::U => 0.84,
    }
}

fn parse_string_field(line: &str, key: &str) -> Option<String> {
    let prefix = format!("\"{}\": \"", key);
    let stripped = line.strip_prefix(&prefix)?;
    let end_idx = stripped.find('"')?;
    Some(stripped[..end_idx].to_string())
}

fn parse_f64_field(line: &str, key: &str) -> Option<f64> {
    let prefix = format!("\"{}\": ", key);
    let stripped = line.strip_prefix(&prefix)?;
    let value = stripped.trim_end_matches(',').trim();
    value.parse::<f64>().ok()
}

fn parse_bool_field(line: &str, key: &str) -> Option<bool> {
    let prefix = format!("\"{}\": ", key);
    let stripped = line.strip_prefix(&prefix)?;
    match stripped.trim_end_matches(',').trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn fingerprint_samples(samples: &[PlayerBucketSample]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| {
        let bucket_cmp = a.bucket.cmp(&b.bucket);
        if bucket_cmp == std::cmp::Ordering::Equal {
            a.tr.total_cmp(&b.tr)
        } else {
            bucket_cmp
        }
    });

    let mut hash = FNV_OFFSET;
    for sample in sorted {
        for byte in sample.bucket.as_rank_str().as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        for byte in sample.tr.to_bits().to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_FIXTURE: &str = r#"{
  "players": [
    {
      "rank": "b",
      "tr": 6900.0,
      "qualified": true
    },
    {
      "rank": "b",
      "tr": 6880.0,
      "qualified": true
    },
    {
      "rank": "u",
      "tr": 22800.0,
      "qualified": true
    },
    {
      "rank": "u",
      "tr": 22750.0,
      "qualified": false
    }
  ]
}"#;

    const MANIFEST_WITH_REPLAY_IDS_FIXTURE: &str = r#"{
  "players": [
    {
      "rank": "b",
      "tr": 6900.0,
      "replay_ids": [
        "one",
        "two"
      ],
      "qualified": true
    },
    {
      "rank": "u",
      "tr": 22800.0,
      "replay_ids": [
        "three"
      ],
      "qualified": true
    }
  ]
}"#;

    #[test]
    fn parse_players_manifest_samples_filters_to_qualified() {
        let samples = parse_players_manifest_samples(MANIFEST_FIXTURE)
            .unwrap_or_else(|e| panic!("parse failed: {e}"));
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].bucket, SkillBucket::B);
        assert_eq!(samples[1].bucket, SkillBucket::B);
        assert_eq!(samples[2].bucket, SkillBucket::U);
    }

    #[test]
    fn parse_players_manifest_samples_handles_replay_id_arrays() {
        let samples = parse_players_manifest_samples(MANIFEST_WITH_REPLAY_IDS_FIXTURE)
            .unwrap_or_else(|e| panic!("parse failed: {e}"));
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].bucket, SkillBucket::B);
        assert_eq!(samples[1].bucket, SkillBucket::U);
    }

    #[test]
    fn generate_profile_is_deterministic_for_same_samples() {
        let profile_a =
            generate_profile_from_players_manifest(CALIBRATION_VERSION_V1, MANIFEST_FIXTURE)
                .unwrap_or_else(|e| panic!("profile failed: {e}"));
        let profile_b =
            generate_profile_from_players_manifest(CALIBRATION_VERSION_V1, MANIFEST_FIXTURE)
                .unwrap_or_else(|e| panic!("profile failed: {e}"));
        assert_eq!(profile_a, profile_b);
        assert_eq!(
            profile_a.to_artifact_string(),
            profile_b.to_artifact_string()
        );
    }

    #[test]
    fn artifact_round_trip_is_stable() {
        let profile =
            generate_profile_from_players_manifest(CALIBRATION_VERSION_V1, MANIFEST_FIXTURE)
                .unwrap_or_else(|e| panic!("profile failed: {e}"));
        let artifact = profile.to_artifact_string();
        let loaded = CalibrationProfile::from_artifact_str(&artifact)
            .unwrap_or_else(|e| panic!("load failed: {e}"));
        assert_eq!(loaded.version, CALIBRATION_VERSION_V1);
        assert_eq!(loaded.source_fingerprint, profile.source_fingerprint);
        assert_eq!(loaded.rows.len(), profile.rows.len());
        assert_eq!(loaded.to_artifact_string(), artifact);
    }

    #[test]
    fn thresholds_validate_increasing_order() {
        let ok = BucketThresholds {
            none_max: 0.6,
            inaccuracy_max: 1.2,
            mistake_max: 2.2,
        };
        assert!(ok.validate().is_ok());

        let bad = BucketThresholds {
            none_max: 1.0,
            inaccuracy_max: 0.5,
            mistake_max: 2.0,
        };
        assert!(bad.validate().is_err());
    }
}
