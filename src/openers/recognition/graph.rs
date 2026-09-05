use std::collections::{HashMap, HashSet};

#[cfg(test)]
use super::census::CompileCensus;
use crate::board::Board;
#[cfg(test)]
use crate::board::LockMechanics;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct CanonicalKey {
    pub(crate) masks: Box<[u16]>,
    pub(crate) letters: Option<Box<[u8]>>,
}

impl CanonicalKey {
    pub(crate) fn from_board(board: &Board) -> Self {
        Self::from_board_with_letters(board, None)
    }

    pub(crate) fn from_board_with_letters(board: &Board, letters: Option<&[u8]>) -> Self {
        let masks = raw_masks(board);
        let letters = letters.map(|letters| &letters[..masks.len() * 10]);
        Self::from_normalized_masks_with_letters(&masks, letters)
    }

    pub(crate) fn from_normalized_rows(masks: &[u16], letters: Option<&[String]>) -> Self {
        let letters = letters.map(|rows| {
            masks
                .iter()
                .enumerate()
                .flat_map(|(row, mask)| {
                    (0..10).map(move |column| {
                        if mask & (1 << column) == 0 {
                            b'_'
                        } else {
                            rows.get(row)
                                .and_then(|row| row.as_bytes().get(column))
                                .copied()
                                .unwrap_or(b'_')
                        }
                    })
                })
                .collect::<Vec<_>>()
        });
        Self::from_normalized_masks_with_letters(masks, letters.as_deref())
    }

    pub(crate) fn from_normalized_masks_with_letters(
        masks: &[u16],
        letters: Option<&[u8]>,
    ) -> Self {
        let masks = masks.to_vec();
        let mirrored = masks.iter().copied().map(mirror_mask).collect::<Vec<_>>();
        let letters = letters.map(|letters| letters.to_vec());
        let mirrored_letters = letters.as_deref().map(mirror_letters);
        let use_mirrored = mirrored < masks || (mirrored == masks && mirrored_letters < letters);
        let (masks, letters) = if use_mirrored {
            (mirrored, mirrored_letters)
        } else {
            (masks, letters)
        };
        Self {
            masks: masks.into_boxed_slice(),
            letters: letters.map(Vec::into_boxed_slice),
        }
    }

    pub(crate) fn occupancy_key(&self) -> Self {
        Self {
            masks: self.masks.clone(),
            letters: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StateOrigin {
    pub(crate) record: Box<str>,
    pub(crate) mirrored: bool,
    pub(crate) node_id: u32,
    pub(crate) placed_subset: u32,
    pub(crate) route_name: Option<Box<str>>,
    pub(crate) bag_complete: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ModelState {
    pub(crate) physical_key: CanonicalKey,
    pub(crate) origins: Vec<StateOrigin>,
    pub(crate) identity_opaque: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StateId(pub(crate) usize);

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EpsilonReason {
    BagBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BridgeReason {
    DirectImpossible,
    BudgetExceeded,
    FrameInconsistent,
}

#[derive(Clone, Debug)]
pub(crate) enum TransitionLabel {
    Lock {
        #[cfg(test)]
        letter: u8,
        #[cfg(test)]
        cells: [[u8; 2]; 4],
        #[cfg(test)]
        cleared_rows: Box<[u8]>,
        #[cfg(test)]
        is_pc: bool,
    },
    Epsilon {
        #[cfg(test)]
        reason: EpsilonReason,
    },
    Bridge {
        #[cfg(test)]
        reason: BridgeReason,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Transition {
    #[cfg(test)]
    pub(crate) from: StateId,
    pub(crate) to: StateId,
    pub(crate) label: TransitionLabel,
}

pub(crate) struct RecognitionGraph {
    pub(crate) states: Vec<ModelState>,
    pub(crate) out: Vec<Vec<Transition>>,
    pub(crate) exact_index: HashMap<CanonicalKey, Box<[StateId]>>,
    #[cfg(test)]
    pub(crate) epsilon_order: Box<[StateId]>,
    #[cfg(test)]
    pub(crate) census: CompileCensus,
}

impl RecognitionGraph {
    #[cfg(test)]
    pub(crate) fn graph_shape(&self) -> (usize, usize, usize, usize) {
        (
            self.states.len(),
            self.out.iter().map(Vec::len).sum(),
            self.exact_index.len(),
            self.epsilon_order.len(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ControlKey {
    pub(crate) record_index: u32,
    pub(crate) mirrored: bool,
    pub(crate) node_id: u32,
    pub(crate) placed_subset: u32,
    pub(crate) bridge: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StateInternKey {
    raw_masks: Box<[u16]>,
    control: ControlKey,
}

pub(crate) struct GraphBuilder {
    states: Vec<ModelState>,
    boards: Vec<Board>,
    out: Vec<Vec<Transition>>,
    interned: HashMap<StateInternKey, StateId>,
    bridged: HashSet<StateId>,
}

impl GraphBuilder {
    pub(crate) fn new() -> Self {
        Self {
            states: Vec::new(),
            boards: Vec::new(),
            out: Vec::new(),
            interned: HashMap::new(),
            bridged: HashSet::new(),
        }
    }

    pub(crate) fn board(&self, state: StateId) -> &Board {
        &self.boards[state.0]
    }

    #[cfg(test)]
    pub(crate) fn states(&self) -> &[ModelState] {
        &self.states
    }

    #[cfg(test)]
    pub(crate) fn out(&self) -> &[Vec<Transition>] {
        &self.out
    }

    pub(crate) fn state_count(&self) -> usize {
        self.states.len()
    }

    pub(crate) fn intern(
        &mut self,
        board: Board,
        control: ControlKey,
        origin: StateOrigin,
        identity_opaque: bool,
    ) -> StateId {
        self.intern_with_physical_key(
            board.clone(),
            CanonicalKey::from_board(&board),
            control,
            origin,
            identity_opaque,
        )
    }

    pub(crate) fn intern_with_physical_key(
        &mut self,
        board: Board,
        physical_key: CanonicalKey,
        control: ControlKey,
        origin: StateOrigin,
        identity_opaque: bool,
    ) -> StateId {
        let raw_masks = raw_masks(&board).into_boxed_slice();
        let intern_key = StateInternKey { raw_masks, control };
        if let Some(state) = self.interned.get(&intern_key).copied() {
            let existing = &mut self.states[state.0];
            if !existing.origins.contains(&origin) {
                existing.origins.push(origin);
            }
            existing.identity_opaque |= identity_opaque;
            return state;
        }

        let state = StateId(self.states.len());
        self.interned.insert(intern_key, state);
        self.states.push(ModelState {
            physical_key,
            origins: vec![origin],
            identity_opaque,
        });
        self.boards.push(board);
        self.out.push(Vec::new());
        state
    }

    pub(crate) fn add_transition(&mut self, from: StateId, to: StateId, label: TransitionLabel) {
        self.out[from.0].push(Transition {
            #[cfg(test)]
            from,
            to,
            label,
        });
    }

    pub(crate) fn mark_bridged(&mut self, state: StateId) {
        self.bridged.insert(state);
    }

    pub(crate) fn is_bridged(&self, state: StateId) -> bool {
        self.bridged.contains(&state)
    }

    pub(crate) fn finish(self) -> RecognitionGraph {
        self.finish_inner(
            #[cfg(test)]
            CompileCensus::new(0, 0, 0, 0),
        )
    }

    #[cfg(test)]
    pub(crate) fn finish_with_census(self, census: CompileCensus) -> RecognitionGraph {
        self.finish_inner(census)
    }

    fn finish_inner(self, #[cfg(test)] census: CompileCensus) -> RecognitionGraph {
        let mut exact_index: HashMap<CanonicalKey, Vec<StateId>> = HashMap::new();
        for (index, state) in self.states.iter().enumerate() {
            let state_id = StateId(index);
            exact_index
                .entry(state.physical_key.occupancy_key())
                .or_default()
                .push(state_id);
        }
        let exact_index = exact_index
            .into_iter()
            .map(|(key, states)| (key, states.into_boxed_slice()))
            .collect();
        #[cfg(test)]
        let epsilon_order = (0..self.states.len()).map(StateId).collect();
        RecognitionGraph {
            states: self.states,
            out: self.out,
            exact_index,
            #[cfg(test)]
            epsilon_order,
            #[cfg(test)]
            census,
        }
    }
}

fn raw_masks(board: &Board) -> Vec<u16> {
    let mut masks = board.rows.to_vec();
    while masks.last().is_some_and(|mask| *mask == 0) {
        masks.pop();
    }
    masks
}

/// Grey terminals use this immediate-clear shadow of declared occupancy for alignment.
pub(crate) fn physical_shadow_of_declared(declared: &Board) -> Board {
    let mut shadow = declared.clone();
    let cleared = shadow.line_clears();
    if cleared != 0 {
        shadow.clear_lines(cleared);
    }
    shadow
}

fn mirror_mask(mask: u16) -> u16 {
    let mut mirrored = 0;
    for x in 0..10 {
        if mask & (1 << x) != 0 {
            mirrored |= 1 << (9 - x);
        }
    }
    mirrored
}

fn mirror_letters(letters: &[u8]) -> Vec<u8> {
    let mut mirrored = vec![b'_'; letters.len()];
    for (row, source) in letters.as_chunks::<10>().0.iter().enumerate() {
        for (column, letter) in source.iter().copied().enumerate() {
            mirrored[row * 10 + (9 - column)] = match letter {
                b'L' => b'J',
                b'J' => b'L',
                b'S' => b'Z',
                b'Z' => b'S',
                other => other,
            };
        }
    }
    mirrored
}

#[cfg(test)]
pub(crate) fn cleared_rows(mechanics: LockMechanics) -> Box<[u8]> {
    (0u8..40)
        .filter(|row| mechanics.cleared_mask & (1u64 << row) != 0)
        .collect()
}
