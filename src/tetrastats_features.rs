#![allow(dead_code)] // TetraStats feature extraction — used by WASM analysis pipeline
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TetraStatsFeatureInput {
    pub apm: f64,
    pub pps: f64,
    pub vs: f64,
    pub statrank: f64,
    pub srarea: f64,
    pub frame_delay_frames: f64,
    pub opener_frames: f64,
    pub placement_attack: f64,
    pub placement_pieces: f64,
    pub opener_attack: f64,
    pub opener_blocks: f64,
    pub lines_cleared: f64,
    pub surge_attack: f64,
    pub surge_lines_cleared: f64,
    pub surge_garbage_cleared: f64,
    pub surge_chains: f64,
    pub surge_btb: f64,
    pub surge_fails: f64,
    pub garbage_lines_received: f64,
    pub cheese_lines_received: f64,
    pub cheese_lines_cancelled: f64,
    pub cheese_lines_tanked: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TetraStatsFeatures {
    pub dsp: f64,
    pub dss: f64,
    pub cheese_index: f64,
    pub surge_rate: f64,
    pub surge_length: f64,
    pub surge_ds: f64,
    pub apl: f64,
    pub opener_apm: f64,
    pub opener_pps: f64,
    pub midgame_apm: f64,
    pub midgame_pps: f64,
    pub opener_axis: f64,
    pub plonk_axis: f64,
    pub stride_axis: f64,
    pub infds_axis: f64,
    pub cheese_lines_received_ratio: f64,
    pub cheese_lines_cancelled_ratio: f64,
    pub cheese_lines_tanked_ratio: f64,
}

fn safe_div(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        let value = numerator / denominator;
        if value.is_finite() {
            value
        } else {
            0.0
        }
    }
}

fn safe_sub_div(numerator: f64, left: f64, right: f64) -> f64 {
    safe_div(numerator, left - right)
}

fn clamp_finite(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

pub(crate) fn extract_whitelist_features(input: &TetraStatsFeatureInput) -> TetraStatsFeatures {
    let app = safe_div(input.apm, input.pps * 60.0);
    let vsapm = safe_div(input.vs, input.apm);
    let dss = safe_div(input.vs, 100.0) - safe_div(input.apm, 60.0);
    let dsp = safe_div(dss, input.pps);
    let cheese_index = (dsp * 150.0) + ((vsapm - 2.0) * 50.0) + (0.6 - app) * 125.0;
    let gbe = app * dsp * 2.0;

    let secs = safe_div(input.frame_delay_frames, 60.0);
    let mins = safe_div(secs, 60.0);
    let attack_secs = safe_div(input.opener_frames, 60.0);
    let attack_mins = safe_div(attack_secs, 60.0);

    let apl = safe_div(input.placement_attack, input.lines_cleared);
    let midgame_apm = safe_sub_div(
        input.placement_attack - input.opener_attack,
        mins,
        attack_mins,
    );
    let midgame_pps = safe_sub_div(
        input.placement_pieces - input.opener_blocks,
        secs,
        attack_secs,
    );
    let opener_apm = safe_div(input.opener_attack, attack_mins);
    let opener_pps = safe_div(input.opener_blocks, attack_secs);

    let surge_rate = safe_div(input.surge_chains, input.surge_chains + input.surge_fails);
    let surge_length = safe_div(input.surge_btb, input.surge_chains);
    let surge_ds = safe_div(input.surge_garbage_cleared, input.surge_chains);

    let statrank = input.statrank;
    let srarea = input.srarea;
    let nmapm = safe_div(
        safe_div(input.apm, srarea),
        (0.069 * 1.0017_f64.powf(statrank.powf(5.0) / 4700.0)) + statrank / 360.0,
    ) - 1.0;
    let nmpps = safe_div(
        safe_div(input.pps, srarea),
        (0.0084264 * 2.14_f64.powf(-2.0 * (statrank / 2.7 + 1.03))) - statrank / 5750.0 + 0.0067,
    ) - 1.0;
    let nmapp = safe_div(
        app,
        (0.1368803292 * 1.0024_f64.powf(statrank.powf(5.0) / 2800.0)) + statrank / 54.0,
    ) - 1.0;
    let nmdsp = safe_div(
        dsp,
        (0.02136327583 * 14.0_f64.powf((statrank - 14.75) / 3.9)) + statrank / 152.0 + 0.022,
    ) - 1.0;
    let nmgbe = safe_div(
        gbe,
        statrank / 350.0 + 0.005948424455 * 3.8_f64.powf((statrank - 6.1) / 4.0) + 0.006,
    ) - 1.0;
    let nmvsapm = safe_div(vsapm, -((statrank - 16.0) / 36.0).powf(2.0) + 2.133) - 1.0;

    let opener_axis =
        ((nmapm + nmpps * 0.75 + nmvsapm * -10.0 + nmapp * 0.75 + nmdsp * -0.25) / 3.5) + 0.5;
    let plonk_axis = ((nmgbe + nmapp + nmdsp * 0.75 - nmpps) / 2.73) + 0.5;
    let stride_axis = ((nmapm * -0.25 + nmpps + nmapp * -2.0 + nmdsp * -0.5) * 0.79) + 0.5;
    let infds_axis =
        ((nmdsp + nmapp * -0.75 + nmapm * 0.5 + nmvsapm * 1.5 + nmpps * 0.5) * 0.9) + 0.5;

    let cheese_lines_received_ratio =
        safe_div(input.cheese_lines_received, input.garbage_lines_received);
    let cheese_lines_cancelled_ratio =
        safe_div(input.cheese_lines_cancelled, input.garbage_lines_received);
    let cheese_lines_tanked_ratio =
        safe_div(input.cheese_lines_tanked, input.garbage_lines_received);

    TetraStatsFeatures {
        dsp: clamp_finite(dsp),
        dss: clamp_finite(dss),
        cheese_index: clamp_finite(cheese_index),
        surge_rate: clamp_finite(surge_rate),
        surge_length: clamp_finite(surge_length),
        surge_ds: clamp_finite(surge_ds),
        apl: clamp_finite(apl),
        opener_apm: clamp_finite(opener_apm),
        opener_pps: clamp_finite(opener_pps),
        midgame_apm: clamp_finite(midgame_apm),
        midgame_pps: clamp_finite(midgame_pps),
        opener_axis: clamp_finite(opener_axis),
        plonk_axis: clamp_finite(plonk_axis),
        stride_axis: clamp_finite(stride_axis),
        infds_axis: clamp_finite(infds_axis),
        cheese_lines_received_ratio: clamp_finite(cheese_lines_received_ratio),
        cheese_lines_cancelled_ratio: clamp_finite(cheese_lines_cancelled_ratio),
        cheese_lines_tanked_ratio: clamp_finite(cheese_lines_tanked_ratio),
    }
}

#[cfg(test)]
mod tetrastats_feature_tests {
    use super::{extract_whitelist_features, TetraStatsFeatureInput};

    fn fixture_input() -> TetraStatsFeatureInput {
        TetraStatsFeatureInput {
            apm: 120.0,
            pps: 2.0,
            vs: 150.0,
            statrank: 12.0,
            srarea: 100.0,
            frame_delay_frames: 3600.0,
            opener_frames: 600.0,
            placement_attack: 120.0,
            placement_pieces: 180.0,
            opener_attack: 30.0,
            opener_blocks: 35.0,
            lines_cleared: 80.0,
            surge_attack: 40.0,
            surge_lines_cleared: 20.0,
            surge_garbage_cleared: 24.0,
            surge_chains: 6.0,
            surge_btb: 18.0,
            surge_fails: 2.0,
            garbage_lines_received: 50.0,
            cheese_lines_received: 20.0,
            cheese_lines_cancelled: 15.0,
            cheese_lines_tanked: 5.0,
        }
    }

    fn approx_eq(actual: f64, expected: f64, epsilon: f64) {
        let delta = (actual - expected).abs();
        assert!(
            delta <= epsilon,
            "expected {expected}, got {actual}, delta={delta}, epsilon={epsilon}"
        );
    }

    #[test]
    fn tetrastats_feature_tests_fixture_parity_matches_reference_formulas() {
        let out = extract_whitelist_features(&fixture_input());

        approx_eq(out.dsp, -0.25, 1e-9);
        approx_eq(out.dss, -0.5, 1e-9);
        approx_eq(out.cheese_index, -125.0, 1e-9);
        approx_eq(out.surge_rate, 0.75, 1e-9);
        approx_eq(out.surge_length, 3.0, 1e-9);
        approx_eq(out.surge_ds, 4.0, 1e-9);
        approx_eq(out.apl, 1.5, 1e-9);
        approx_eq(out.opener_apm, 180.0, 1e-9);
        approx_eq(out.opener_pps, 3.5, 1e-9);
        approx_eq(out.midgame_apm, 108.0, 1e-9);
        approx_eq(out.midgame_pps, 2.9, 1e-9);
        approx_eq(out.opener_axis, 5.827778757702805, 1e-12);
        approx_eq(out.plonk_axis, -3.660945919880196, 1e-12);
        approx_eq(out.stride_axis, 0.04060821686821797, 1e-12);
        approx_eq(out.infds_axis, 1.8513823503067273, 1e-12);
        approx_eq(out.cheese_lines_received_ratio, 0.4, 1e-9);
        approx_eq(out.cheese_lines_cancelled_ratio, 0.3, 1e-9);
        approx_eq(out.cheese_lines_tanked_ratio, 0.1, 1e-9);
    }

    #[test]
    fn tetrastats_feature_tests_zero_denominator_paths_return_finite_zeroes() {
        let input = TetraStatsFeatureInput {
            apm: 0.0,
            pps: 0.0,
            vs: 0.0,
            statrank: 0.0,
            srarea: 0.0,
            frame_delay_frames: 0.0,
            opener_frames: 0.0,
            placement_attack: 0.0,
            placement_pieces: 0.0,
            opener_attack: 0.0,
            opener_blocks: 0.0,
            lines_cleared: 0.0,
            surge_attack: 0.0,
            surge_lines_cleared: 0.0,
            surge_garbage_cleared: 0.0,
            surge_chains: 0.0,
            surge_btb: 0.0,
            surge_fails: 0.0,
            garbage_lines_received: 0.0,
            cheese_lines_received: 0.0,
            cheese_lines_cancelled: 0.0,
            cheese_lines_tanked: 0.0,
        };
        let out = extract_whitelist_features(&input);

        let outputs = [
            out.dsp,
            out.dss,
            out.cheese_index,
            out.surge_rate,
            out.surge_length,
            out.surge_ds,
            out.apl,
            out.opener_apm,
            out.opener_pps,
            out.midgame_apm,
            out.midgame_pps,
            out.opener_axis,
            out.plonk_axis,
            out.stride_axis,
            out.infds_axis,
            out.cheese_lines_received_ratio,
            out.cheese_lines_cancelled_ratio,
            out.cheese_lines_tanked_ratio,
        ];

        for value in outputs {
            assert!(value.is_finite(), "value should be finite: {value}");
        }

        assert_eq!(out.dsp, 0.0);
        assert_eq!(out.dss, 0.0);
        assert_eq!(out.cheese_index, -25.0);
        assert_eq!(out.surge_rate, 0.0);
        assert_eq!(out.surge_length, 0.0);
        assert_eq!(out.surge_ds, 0.0);
        assert_eq!(out.apl, 0.0);
    }
}
