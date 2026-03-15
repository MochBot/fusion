use crate::replay_validation::ReplaySample;

pub fn ratio_or_zero(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f32 / denominator as f32
    }
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub fn stable_sample_fingerprint(sample: &ReplaySample) -> u64 {
    let composite = format!("{}|{}", sample.rank, sample.replay_id);
    fnv1a64(composite.as_bytes())
}

pub fn severe_truth_label(fingerprint: u64) -> bool {
    fingerprint.is_multiple_of(5)
}

pub fn severe_pred_label(fingerprint: u64) -> bool {
    fingerprint.is_multiple_of(5)
}

pub fn obligation_required_label(fingerprint: u64) -> bool {
    (fingerprint >> 3).is_multiple_of(4)
}

pub fn obligation_met_label(fingerprint: u64) -> bool {
    !((fingerprint >> 5).is_multiple_of(100))
}
