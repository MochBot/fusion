//! Per-candidate exact-attack corpus generator (parity target: rank-dump-k7-attack.ts).
//!
//! For each decision position it emits one fixed-size record per legal first-ply
//! afterstate: packed board rows + garbage mask, the afterstate piece context,
//! post-move b2b/combo/pending/mult, the immediate S2 attack, and the EXACT
//! K-horizon attack (immediate + best continuation over the remaining K-1 pieces).
//! `groups.json` carries per-group {start,size,best,player,oracle_mode,matched}.

use std::env;
use std::fs::{create_dir_all, File};
use std::io::{self, Read, Write};

use rayon::prelude::*;

use fusion_engine::label_kernel::{
    beam_best, expand_candidates, inject_garbage, parse_context, reconstruct, record_bytes,
    ContextRec,
};

const DEFAULT_K: usize = 7;
const DEFAULT_BEAM: usize = 300;
const REC: usize = 195;

const OFF_ROWS: usize = 0;
const OFF_GMASK: usize = 80;
const OFF_PIECE: usize = 160;
const OFF_HOLD: usize = 161;
const OFF_QUEUE: usize = 162;
const OFF_B2B: usize = 167;
const OFF_COMBO: usize = 171;
const OFF_PENDING: usize = 175;
const OFF_MULT: usize = 179;
const OFF_IMMEDIATE: usize = 183;
const OFF_EXACT: usize = 187;
const OFF_GROUP: usize = 191;

struct PositionOut {
    records: Vec<[u8; REC]>,
    best: f32,
    player: f32,
    matched: bool,
}

fn put_u16(rec: &mut [u8; REC], off: usize, value: u16) {
    rec[off..off + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_i32(rec: &mut [u8; REC], off: usize, value: i32) {
    rec[off..off + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_f32(rec: &mut [u8; REC], off: usize, value: f32) {
    rec[off..off + 4].copy_from_slice(&value.to_le_bytes());
}

fn label_position(ctx: &ContextRec, beam: usize) -> Option<PositionOut> {
    let k = ctx.frames.len().checked_sub(1)?;
    let pieces: Vec<i8> = ctx
        .frames
        .iter()
        .take(k)
        .map(|f| f.piece)
        .filter(|&p| p >= 0)
        .collect();
    if pieces.is_empty() || ctx.frames[0].piece < 0 {
        return None;
    }
    let f0 = &ctx.frames[0];
    let candidates = expand_candidates(f0, pieces[0]);
    if candidates.is_empty() {
        return None;
    }

    let recon = reconstruct(&ctx.frames[0..=pieces.len()]);
    let matched = recon.is_some();

    let player = match &recon {
        Some((acc, _, _)) => *acc as f32,
        None => ctx.frames[0..pieces.len()]
            .iter()
            .map(|f| f.atk)
            .sum::<f64>() as f32,
    };

    let mult_full: Vec<f64> = ctx.frames[0..pieces.len()].iter().map(|f| f.mult).collect();
    let keep_full: Vec<[u16; 40]> = ctx.frames[1..=pieces.len()]
        .iter()
        .map(|f| f.rows)
        .collect();
    let best = match &recon {
        Some((_, gc, gr)) => beam_best(
            &f0.rows,
            &f0.gmask,
            &pieces,
            f0.b2b.max(0),
            f0.combo.max(0),
            0,
            Some(&keep_full),
            &mult_full,
            Some(gc),
            Some(gr),
            beam,
            false,
        ),
        None => {
            let mult = vec![f0.mult; pieces.len()];
            beam_best(
                &f0.rows,
                &f0.gmask,
                &pieces,
                f0.b2b.max(0),
                f0.combo.max(0),
                f0.pending.max(0),
                None,
                &mult,
                None,
                None,
                beam,
                false,
            )
        }
    };

    let cont_pieces: Vec<i8> = pieces[1..].to_vec();
    let cont_mult: Vec<f64> = ctx.frames[1..pieces.len()].iter().map(|f| f.mult).collect();
    let cont_keep: Vec<[u16; 40]> = ctx.frames[2..=pieces.len()]
        .iter()
        .map(|f| f.rows)
        .collect();

    let after_piece = pieces.get(1).copied().unwrap_or(-1);
    let mut after_queue = [-1i8; 5];
    for (i, slot) in after_queue.iter_mut().enumerate() {
        *slot = pieces.get(2 + i).copied().unwrap_or(-1);
    }

    let records: Vec<[u8; REC]> = candidates
        .iter()
        .map(|c| {
            let continuation = if cont_pieces.is_empty() {
                0.0
            } else if let Some((_, gc, gr)) = &recon {
                let (inj_rows, inj_gmask) = inject_garbage(&c.rows, &c.gmask, gc[0], &gr[0]);
                beam_best(
                    &inj_rows,
                    &inj_gmask,
                    &cont_pieces,
                    c.b2b.max(0),
                    c.combo.max(0),
                    0,
                    Some(&cont_keep),
                    &cont_mult,
                    Some(&gc[1..]),
                    Some(&gr[1..]),
                    beam,
                    false,
                )
            } else {
                let mult = vec![f0.mult; cont_pieces.len()];
                beam_best(
                    &c.rows,
                    &c.gmask,
                    &cont_pieces,
                    c.b2b.max(0),
                    c.combo.max(0),
                    0,
                    None,
                    &mult,
                    None,
                    None,
                    beam,
                    false,
                )
            };
            let exact = c.immediate + continuation;

            let mut rec = [0u8; REC];
            for y in 0..40 {
                put_u16(&mut rec, OFF_ROWS + y * 2, c.rows[y]);
                put_u16(&mut rec, OFF_GMASK + y * 2, c.gmask[y]);
            }
            rec[OFF_PIECE] = after_piece as u8;
            rec[OFF_HOLD] = f0.hold as u8;
            for i in 0..5 {
                rec[OFF_QUEUE + i] = after_queue[i] as u8;
            }
            put_i32(&mut rec, OFF_B2B, c.b2b);
            put_i32(&mut rec, OFF_COMBO, c.combo);
            put_i32(&mut rec, OFF_PENDING, ctx.frames[0].pending);
            put_f32(&mut rec, OFF_MULT, f0.mult as f32);
            put_f32(&mut rec, OFF_IMMEDIATE, c.immediate as f32);
            put_f32(&mut rec, OFF_EXACT, exact as f32);
            put_f32(&mut rec, OFF_GROUP, 0.0);
            rec
        })
        .collect();

    Some(PositionOut {
        records,
        best: best as f32,
        player,
        matched,
    })
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

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

fn main() -> io::Result<()> {
    let k = env_usize("K", DEFAULT_K);
    let beam = env_usize("BEAM", DEFAULT_BEAM);
    let max_positions = env_usize("MAX_POSITIONS", usize::MAX);
    let out_dir = env::var("OUT_DIR")
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "OUT_DIR required"))?;

    let input = read_input()?;
    let rb = record_bytes(k);
    if input.len() % rb != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("input size {} not a multiple of {rb}", input.len()),
        ));
    }
    let total = input.len() / rb;
    let take = total.min(max_positions);

    let outs: Vec<Option<PositionOut>> = (0..take)
        .into_par_iter()
        .map(|i| label_position(&parse_context(&input[i * rb..(i + 1) * rb], k), beam))
        .collect();

    create_dir_all(&out_dir)?;
    let mut cand = Vec::new();
    let mut groups_json = String::from("[");
    let mut start = 0usize;
    let mut gi_groups = 0usize;
    let mut fallback_groups = 0usize;
    let mut group_index: u32 = 0;
    for pos in outs.into_iter().flatten() {
        let size = pos.records.len();
        if pos.matched {
            gi_groups += 1;
        } else {
            fallback_groups += 1;
        }
        for mut rec in pos.records {
            put_f32(&mut rec, OFF_GROUP, f32::from_bits(group_index));
            cand.extend_from_slice(&rec);
        }
        if group_index > 0 {
            groups_json.push(',');
        }
        groups_json.push_str(&format!(
            "{{\"start\":{},\"size\":{},\"best\":{},\"player\":{},\"oracle_mode\":\"{}\",\"matched\":{}}}",
            start, size, pos.best, pos.player, if pos.matched { "gi" } else { "fallback" }, pos.matched,
        ));
        start += size;
        group_index += 1;
    }
    groups_json.push(']');

    File::create(format!("{out_dir}/cand.bin"))?.write_all(&cand)?;
    let meta = format!(
        "{{\"record_bytes\":{REC},\"rows_off\":{OFF_ROWS},\"gmask_off\":{OFF_GMASK},\"piece_off\":{OFF_PIECE},\"hold_off\":{OFF_HOLD},\"queue_off\":{OFF_QUEUE},\"b2b_off\":{OFF_B2B},\"combo_off\":{OFF_COMBO},\"pending_off\":{OFF_PENDING},\"mult_off\":{OFF_MULT},\"immediate_off\":{OFF_IMMEDIATE},\"exact_off\":{OFF_EXACT},\"group_off\":{OFF_GROUP},\"positions\":{group_index},\"totalCands\":{start},\"groups\":{groups_json}}}"
    );
    File::create(format!("{out_dir}/groups.json"))?.write_all(meta.as_bytes())?;
    eprintln!("label_opp_cand positions={group_index} totalCands={start} giGroups={gi_groups} fallbackGroups={fallback_groups}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_engine::label_kernel::{expand_candidates, ContextRec};

    fn empty_k7_ctx() -> ContextRec {
        let mut ctx = ContextRec::with_horizon(7);
        let seq = [0i8, 1, 2, 3, 4, 5, 6, 0];
        for (i, frame) in ctx.frames.iter_mut().enumerate() {
            frame.piece = seq[i];
            frame.hold = -1;
            frame.queue = [
                seq[(i + 1) % 8],
                seq[(i + 2) % 8],
                seq[(i + 3) % 8],
                seq[(i + 4) % 8],
                seq[(i + 5) % 8],
            ];
            frame.mult = 1.0;
        }
        ctx.opp_mult = 1.0;
        ctx
    }

    #[test]
    fn record_size_is_195() {
        assert_eq!(REC, 195);
        assert_eq!(OFF_GROUP + 4, REC);
    }

    #[test]
    fn group_size_matches_candidate_count() {
        let ctx = empty_k7_ctx();
        let out = label_position(&ctx, 6).expect("position labeled");
        let cands = expand_candidates(&ctx.frames[0], ctx.frames[0].piece);
        assert_eq!(out.records.len(), cands.len());
        assert!(!out.records.is_empty());
    }

    #[test]
    fn exact_decomposes_into_immediate_plus_continuation() {
        let ctx = empty_k7_ctx();
        let out = label_position(&ctx, 6).expect("position labeled");
        let pieces: Vec<i8> = ctx.frames.iter().take(7).map(|f| f.piece).collect();
        let cont_pieces: Vec<i8> = pieces[1..].to_vec();
        let cands = expand_candidates(&ctx.frames[0], pieces[0]);
        // empty board never reconstructs -> fallback continuation
        for (rec, c) in out.records.iter().zip(cands.iter()) {
            let immediate =
                f32::from_le_bytes(rec[OFF_IMMEDIATE..OFF_IMMEDIATE + 4].try_into().unwrap());
            let exact = f32::from_le_bytes(rec[OFF_EXACT..OFF_EXACT + 4].try_into().unwrap());
            let mult = vec![ctx.frames[0].mult; cont_pieces.len()];
            let cont = beam_best(
                &c.rows,
                &c.gmask,
                &cont_pieces,
                c.b2b.max(0),
                c.combo.max(0),
                0,
                None,
                &mult,
                None,
                None,
                6,
                false,
            );
            assert!((exact - (immediate + cont as f32)).abs() < 1e-3);
        }
    }
}
