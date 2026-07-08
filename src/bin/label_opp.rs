//! Position-level opponent-context labeler.
//!
//! Reads `.ctx` records and emits one `STRIDE_F`-float row per decision position
//! (own board/onehots + opp block + labels best/playerAtk/gap/outcome/matched).
//! All beam/expansion/reconstruction logic lives in `fusion_engine::label_kernel`
//! and is shared byte-identically with the per-candidate `label_opp_cand` bin.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::sync::atomic::Ordering;

use rayon::prelude::*;

use fusion_engine::label_kernel::{
    beam_best, height_holes, parse_context, reconstruct, ContextRec, PROFILE,
};

const DEFAULT_K: usize = 5;
const STRIDE_F: usize = 464;
const STRIDE_BYTES: usize = STRIDE_F * 4;
const DEFAULT_BEAM: usize = 300;
const DEFAULT_BEAM_FEAT: usize = 150;

#[derive(Clone, Copy)]
struct LabelConfig {
    k: usize,
    beam: usize,
    beam_feat: usize,
    profile: bool,
}

impl LabelConfig {
    fn new(k: usize, beam: usize, beam_feat: usize) -> Self {
        Self {
            k,
            beam,
            beam_feat,
            profile: false,
        }
    }

    fn from_env() -> io::Result<Self> {
        let mut cfg = Self::new(
            read_positive_usize_env("K", DEFAULT_K)?,
            read_positive_usize_env("BEAM", DEFAULT_BEAM)?,
            read_positive_usize_env("BEAM_FEAT", DEFAULT_BEAM_FEAT)?,
        );
        cfg.profile = env::var_os("LABEL_OPP_PROFILE").is_some();
        Ok(cfg)
    }

    fn record_bytes(self) -> usize {
        fusion_engine::label_kernel::record_bytes(self.k)
    }
}

fn read_positive_usize_env(name: &str, default: usize) -> io::Result<usize> {
    let Some(raw) = env::var_os(name) else {
        return Ok(default);
    };
    let raw = raw.to_string_lossy();
    let value = raw.parse::<usize>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be a positive integer, got {raw}"),
        )
    })?;
    if value == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be positive"),
        ));
    }
    Ok(value)
}

struct AttackInput<'a> {
    rows: &'a [u16; 40],
    gmask: &'a [u16; 40],
    pieces: &'a [i8],
    k: usize,
    b2b: i32,
    combo: i32,
    pending: i32,
    mult: f64,
    beam_feat: usize,
    profile: bool,
}

fn feat_attack(input: AttackInput<'_>) -> f64 {
    if input.pieces.len() < input.k || input.pieces.iter().any(|&p| p < 0) {
        return 0.0;
    }
    let pieces = &input.pieces[..input.k];
    let multipliers = vec![input.mult; pieces.len()];
    beam_best(
        input.rows,
        input.gmask,
        pieces,
        input.b2b.max(0),
        input.combo.max(0),
        input.pending.max(0),
        None,
        &multipliers,
        None,
        None,
        input.beam_feat,
        input.profile,
    )
}

fn put_f32(out: &mut [u8; STRIDE_BYTES], off: &mut usize, value: f32) {
    out[*off..*off + 4].copy_from_slice(&value.to_le_bytes());
    *off += 4;
}

fn label_record(ctx: &ContextRec, cfg: &LabelConfig) -> [u8; STRIDE_BYTES] {
    let rec = &ctx.frames[0];
    let pieces: Vec<i8> = ctx
        .frames
        .iter()
        .take(cfg.k)
        .map(|frame| frame.piece)
        .collect();
    let multipliers: Vec<f64> = ctx
        .frames
        .iter()
        .take(cfg.k)
        .map(|frame| frame.mult)
        .collect();
    let recon = reconstruct(&ctx.frames);
    let matched = if recon.is_some() { 1.0 } else { 0.0 };
    let player_atk = recon
        .as_ref()
        .map(|r| r.0)
        .unwrap_or_else(|| ctx.frames[..cfg.k].iter().map(|f| f.atk).sum());
    let best = if let Some((_, garbage_counts, garbage_rows)) = &recon {
        let keep: Vec<[u16; 40]> = ctx
            .frames
            .iter()
            .skip(1)
            .take(cfg.k)
            .map(|frame| frame.rows)
            .collect();
        beam_best(
            &rec.rows,
            &rec.gmask,
            &pieces,
            rec.b2b,
            rec.combo,
            rec.pending,
            Some(&keep),
            &multipliers,
            Some(garbage_counts),
            Some(garbage_rows),
            cfg.beam,
            cfg.profile,
        )
    } else {
        let fallback_multipliers = vec![rec.mult; cfg.k];
        beam_best(
            &rec.rows,
            &rec.gmask,
            &pieces,
            rec.b2b,
            rec.combo,
            rec.pending,
            None,
            &fallback_multipliers,
            None,
            None,
            cfg.beam,
            cfg.profile,
        )
    };
    let my_pieces = [
        rec.piece,
        rec.queue[0],
        rec.queue[1],
        rec.queue[2],
        rec.queue[3],
    ];
    let opp_pieces = [
        ctx.opp_piece,
        ctx.opp_queue[0],
        ctx.opp_queue[1],
        ctx.opp_queue[2],
        ctx.opp_queue[3],
    ];
    let my_best = feat_attack(AttackInput {
        rows: &rec.rows,
        gmask: &rec.gmask,
        pieces: &my_pieces,
        k: cfg.k,
        b2b: rec.b2b,
        combo: rec.combo,
        pending: rec.pending,
        mult: rec.mult,
        beam_feat: cfg.beam_feat,
        profile: cfg.profile,
    })
    .max(0.0);
    let opp_best = feat_attack(AttackInput {
        rows: &ctx.opp_rows,
        gmask: &ctx.opp_gmask,
        pieces: &opp_pieces,
        k: cfg.k,
        b2b: ctx.opp_b2b,
        combo: ctx.opp_combo,
        pending: ctx.opp_pending,
        mult: ctx.opp_mult,
        beam_feat: cfg.beam_feat,
        profile: cfg.profile,
    })
    .max(0.0);
    let (oh, ohl) = height_holes(&ctx.opp_rows);

    let mut out = [0u8; STRIDE_BYTES];
    let mut off = 0;
    for row in &rec.rows {
        for x in 0..10 {
            put_f32(
                &mut out,
                &mut off,
                if *row & (1u16 << x) != 0 { 1.0 } else { 0.0 },
            );
        }
    }
    for p in 0..7 {
        put_f32(&mut out, &mut off, if rec.piece == p { 1.0 } else { 0.0 });
    }
    for p in 0..7 {
        put_f32(&mut out, &mut off, if rec.hold == p { 1.0 } else { 0.0 });
    }
    for queued in &rec.queue {
        for p in 0..7 {
            put_f32(&mut out, &mut off, if *queued == p { 1.0 } else { 0.0 });
        }
    }
    put_f32(&mut out, &mut off, rec.b2b.max(0) as f32);
    put_f32(&mut out, &mut off, rec.combo.max(0) as f32);
    put_f32(&mut out, &mut off, ctx.opp_b2b.max(0) as f32);
    put_f32(&mut out, &mut off, ctx.opp_combo.max(0) as f32);
    put_f32(&mut out, &mut off, ctx.opp_pending as f32);
    put_f32(&mut out, &mut off, oh as f32);
    put_f32(&mut out, &mut off, ohl as f32);
    put_f32(&mut out, &mut off, rec.pending as f32);
    put_f32(&mut out, &mut off, my_best as f32);
    put_f32(&mut out, &mut off, opp_best as f32);
    put_f32(&mut out, &mut off, best as f32);
    put_f32(&mut out, &mut off, player_atk as f32);
    put_f32(&mut out, &mut off, (best - player_atk) as f32);
    put_f32(&mut out, &mut off, ctx.outcome as f32);
    put_f32(&mut out, &mut off, matched as f32);
    debug_assert_eq!(off, STRIDE_BYTES);
    out
}

fn read_input() -> io::Result<Vec<u8>> {
    let mut input = Vec::new();
    if let Some(path) = env::args().nth(1) {
        File::open(path)?.read_to_end(&mut input)?;
    } else {
        io::stdin().read_to_end(&mut input)?;
    }
    Ok(input)
}

fn validate_input_size(len: usize, cfg: &LabelConfig) -> io::Result<()> {
    let record_bytes = cfg.record_bytes();
    if !len.is_multiple_of(record_bytes) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("input size {len} is not a multiple of {record_bytes}"),
        ));
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let cfg = LabelConfig::from_env()?;
    let input = read_input()?;
    validate_input_size(input.len(), &cfg)?;
    let bench = env::var_os("LABEL_OPP_BENCH").is_some();
    let t0 = std::time::Instant::now();
    let record_bytes = cfg.record_bytes();
    let records: Vec<[u8; STRIDE_BYTES]> = input
        .par_chunks_exact(record_bytes)
        .map(|chunk| label_record(&parse_context(chunk, cfg.k), &cfg))
        .collect();
    let mut stdout = io::stdout().lock();
    for record in &records {
        stdout.write_all(record)?;
    }
    if bench {
        let dt = t0.elapsed().as_secs_f64();
        eprintln!(
            "label_opp records={} samples/s={:.2}",
            records.len(),
            records.len() as f64 / dt
        );
    }
    if cfg.profile {
        eprintln!(
            "label_opp_profile beam_calls={} beam_nodes={} moves={} unique={} generate_ms={:.3} score_ms={:.3} prune_ms={:.3}",
            PROFILE.beam_calls.load(Ordering::Relaxed),
            PROFILE.beam_nodes.load(Ordering::Relaxed),
            PROFILE.moves.load(Ordering::Relaxed),
            PROFILE.generated_unique.load(Ordering::Relaxed),
            PROFILE.generate_ns.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            PROFILE.score_ns.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            PROFILE.prune_ns.load(Ordering::Relaxed) as f64 / 1_000_000.0,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_engine::label_kernel::ContextRec;

    #[test]
    fn context_record_size_matches_emitter_schema() {
        assert_eq!(LabelConfig::new(5, 300, 150).record_bytes(), 1360);
        assert_eq!(STRIDE_BYTES, 1856);
    }

    #[test]
    fn k7_context_record_size_matches_emitter_schema() {
        let cfg = LabelConfig::new(7, 500, 150);
        assert_eq!(cfg.record_bytes(), 1750);
        assert_eq!(STRIDE_BYTES, 1856);
    }

    #[test]
    fn empty_context_labels_have_expected_static_features() {
        let mut ctx = ContextRec::default();
        for frame in &mut ctx.frames {
            frame.piece = 0;
            frame.hold = 1;
            frame.queue = [2, 3, 4, 5, 6];
            frame.mult = 1.0;
        }
        ctx.opp_piece = 0;
        ctx.opp_queue = [1, 2, 3, 4, 5];
        ctx.opp_mult = 1.0;
        let out = label_record(&ctx, &LabelConfig::new(5, 300, 150));
        assert_eq!(out.len(), STRIDE_BYTES);
        let piece0 = f32::from_le_bytes(out[400 * 4..401 * 4].try_into().unwrap());
        assert_eq!(piece0, 1.0);
    }

    #[test]
    fn k7_empty_context_labels_have_expected_static_features() {
        let cfg = LabelConfig::new(7, 500, 150);
        let mut ctx = ContextRec::with_horizon(cfg.k);
        for frame in &mut ctx.frames {
            frame.piece = 0;
            frame.hold = 1;
            frame.queue = [2, 3, 4, 5, 6];
            frame.mult = 1.0;
        }
        ctx.opp_piece = 0;
        ctx.opp_queue = [1, 2, 3, 4, 5];
        ctx.opp_mult = 1.0;
        let out = label_record(&ctx, &cfg);
        assert_eq!(out.len(), STRIDE_BYTES);
        let piece0 = f32::from_le_bytes(out[400 * 4..401 * 4].try_into().unwrap());
        assert_eq!(piece0, 1.0);
    }

    #[test]
    fn input_size_validation_uses_configured_horizon() {
        let cfg = LabelConfig::new(7, 500, 150);
        let err = validate_input_size(cfg.record_bytes() - 1, &cfg).unwrap_err();
        assert!(err.to_string().contains("1750"));
    }
}
