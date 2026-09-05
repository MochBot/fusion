use serde::{Deserialize, Serialize};

use crate::openers::board::OPENER_BOARD_WIDTH;

use super::graph::CanonicalKey;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditCosts {
    /// Authored synchronous score, fixed at zero.
    pub(crate) synchronous: u32,
    /// A differing pair of known letters on shared occupancy receives this score.
    pub(crate) substitute_letter: u32,
    /// Each occupied-on-one-side cell receives this score.
    pub(crate) cell_mismatch: u32,
    /// Each observed lock without a consumed model lock receives this score.
    pub(crate) observed_only: u32,
    /// Each consumed model lock without an observation receives this score.
    pub(crate) model_only: u32,
    /// Entry to an identity-opaque model terminal receives this score.
    pub(crate) identity_opaque_entry: u32,
    /// Each unknown hypothesis lock receives this score.
    pub(crate) unknown_per_lock: u32,
}

impl Default for EditCosts {
    fn default() -> Self {
        Self {
            synchronous: 0,
            substitute_letter: 3,
            cell_mismatch: 1,
            observed_only: 5,
            model_only: 4,
            identity_opaque_entry: 2,
            unknown_per_lock: 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditOps {
    pub(crate) synchronous: u16,
    pub(crate) substitutions: u16,
    pub(crate) cell_mismatches: u16,
    pub(crate) observed_only: u16,
    pub(crate) model_only: u16,
}

impl EditOps {
    const fn synchronous() -> Self {
        Self {
            synchronous: 1,
            substitutions: 0,
            cell_mismatches: 0,
            observed_only: 0,
            model_only: 0,
        }
    }
}

pub(crate) fn edit_cost(
    observed: &CanonicalKey,
    expected: &CanonicalKey,
    costs: &EditCosts,
) -> (u32, EditOps) {
    if observed == expected {
        return (0, EditOps::synchronous());
    }

    let mut ops = EditOps {
        synchronous: 0,
        substitutions: 0,
        cell_mismatches: 0,
        observed_only: 0,
        model_only: 0,
    };
    let rows = observed.masks.len().max(expected.masks.len());
    let row_mask = (1u16 << OPENER_BOARD_WIDTH) - 1;
    let mut mismatches = 0u32;
    let mut substitutions = 0u32;
    for row in 0..rows {
        let observed_mask = observed.masks.get(row).copied().unwrap_or_default() & row_mask;
        let expected_mask = expected.masks.get(row).copied().unwrap_or_default() & row_mask;
        mismatches += (observed_mask ^ expected_mask).count_ones();
        let mut shared = observed_mask & expected_mask;
        if shared == 0 {
            continue;
        }
        let (Some(observed_letters), Some(expected_letters)) =
            (observed.letters.as_deref(), expected.letters.as_deref())
        else {
            continue;
        };
        while shared != 0 {
            let column = shared.trailing_zeros() as usize;
            shared &= shared - 1;
            let index = row * OPENER_BOARD_WIDTH + column;
            let (Some(observed_letter), Some(expected_letter)) = (
                observed_letters
                    .get(index)
                    .copied()
                    .filter(|letter| *letter != b'_'),
                expected_letters
                    .get(index)
                    .copied()
                    .filter(|letter| *letter != b'_'),
            ) else {
                continue;
            };
            if observed_letter != expected_letter {
                substitutions += 1;
            }
        }
    }
    ops.cell_mismatches = u16::try_from(mismatches).unwrap_or(u16::MAX);
    ops.substitutions = u16::try_from(substitutions).unwrap_or(u16::MAX);

    let score = u32::from(ops.cell_mismatches)
        .saturating_mul(costs.cell_mismatch)
        .saturating_add(u32::from(ops.substitutions).saturating_mul(costs.substitute_letter));
    (score, ops)
}

#[cfg(test)]
mod tests {
    use super::{edit_cost, EditCosts, EditOps};
    use crate::openers::recognition::graph::CanonicalKey;

    #[test]
    fn edit_cost_is_zero_and_synchronous_for_identical_keys() {
        let key = canonical_key(&[0b0000000001], None);

        let (score, ops) = edit_cost(&key, &key, &EditCosts::default());

        assert_eq!(score, 0);
        assert_eq!(ops, EditOps::synchronous());
    }

    #[test]
    fn edit_cost_keeps_identical_keys_at_zero() {
        let key = canonical_key(&[0b0000000001], None);
        let costs = EditCosts {
            synchronous: 99,
            ..EditCosts::default()
        };

        let (score, ops) = edit_cost(&key, &key, &costs);

        assert_eq!(score, 0);
        assert_eq!(ops, EditOps::synchronous());
    }

    #[test]
    fn edit_cost_counts_one_changed_cell() {
        let observed = canonical_key(&[0b0000000001], None);
        let expected = canonical_key(&[], None);

        let (score, ops) = edit_cost(&observed, &expected, &EditCosts::default());

        assert_eq!(score, 1);
        assert_eq!(
            ops,
            EditOps {
                synchronous: 0,
                substitutions: 0,
                cell_mismatches: 1,
                observed_only: 0,
                model_only: 0,
            }
        );
    }

    #[test]
    fn edit_cost_counts_known_letter_disagreement_on_shared_occupancy() {
        let observed = canonical_key(&[0b0000000001], Some(&[b'I']));
        let expected = canonical_key(&[0b0000000001], Some(&[b'O']));

        let (score, ops) = edit_cost(&observed, &expected, &EditCosts::default());

        assert_eq!(score, 3);
        assert_eq!(
            ops,
            EditOps {
                synchronous: 0,
                substitutions: 1,
                cell_mismatches: 0,
                observed_only: 0,
                model_only: 0,
            }
        );
    }

    #[test]
    fn edit_cost_treats_a_missing_letter_as_unknown_not_a_substitution() {
        let observed = canonical_key(&[0b0000000001], Some(&[b'I']));
        let expected = canonical_key(&[0b0000000001], None);

        let (score, ops) = edit_cost(&observed, &expected, &EditCosts::default());

        assert_eq!(score, 0);
        assert_eq!(
            ops,
            EditOps {
                synchronous: 0,
                substitutions: 0,
                cell_mismatches: 0,
                observed_only: 0,
                model_only: 0,
            }
        );
    }

    #[test]
    fn edit_cost_treats_absent_rows_as_empty() {
        let observed = canonical_key(&[0b0000000001, 0b0000000010], None);
        let expected = canonical_key(&[0b0000000001], None);

        let (score, ops) = edit_cost(&observed, &expected, &EditCosts::default());

        assert_eq!(score, 1);
        assert_eq!(
            ops,
            EditOps {
                synchronous: 0,
                substitutions: 0,
                cell_mismatches: 1,
                observed_only: 0,
                model_only: 0,
            }
        );
    }

    fn canonical_key(masks: &[u16], letters: Option<&[u8]>) -> CanonicalKey {
        CanonicalKey {
            masks: masks.into(),
            letters: letters.map(Into::into),
        }
    }
}
