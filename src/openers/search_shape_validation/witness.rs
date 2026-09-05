use crate::header::{Move, Rotation, SpinType};

use super::flow::FlowTransition;
use super::model::{piece_name, Placement, WitnessLock};

pub(super) fn witness_lock(
    ordinal: u32,
    target: Move,
    rows_before_clear: [u16; 40],
    rows_after: [u16; 40],
    flow: &FlowTransition,
) -> Option<WitnessLock> {
    Some(WitnessLock {
        ordinal,
        piece: piece_name(target.piece()),
        current_before: piece_name(flow.current_before),
        hold_before: flow.hold_before.map(piece_name),
        hold_used: flow.hold_used,
        hold_after: flow.hold_after.map(piece_name),
        draws_before_lock: flow.draws_before_lock.clone(),
        placement: Placement {
            rotation: rotation_name(target.rotation()),
            x: target.x(),
            y: target.y(),
            spin: spin_name(target.spin()),
            cells: move_cells(target)?,
        },
        rows_before_clear: board_rows(rows_before_clear),
        rows_after: board_rows(rows_after),
    })
}

pub(super) fn move_cells(target: Move) -> Option<[[u8; 2]; 4]> {
    let offsets = target.cells();
    let cells = [
        (target.x(), target.y()),
        (
            target.x() + i32::from(offsets[0].x),
            target.y() + i32::from(offsets[0].y),
        ),
        (
            target.x() + i32::from(offsets[1].x),
            target.y() + i32::from(offsets[1].y),
        ),
        (
            target.x() + i32::from(offsets[2].x),
            target.y() + i32::from(offsets[2].y),
        ),
    ];
    let mut output = [[0; 2]; 4];
    for (index, (x, y)) in cells.into_iter().enumerate() {
        output[index] = [u8::try_from(x).ok()?, u8::try_from(y).ok()?];
    }
    Some(output)
}

pub(super) fn board_rows(rows: [u16; 40]) -> Vec<String> {
    let Some(last) = rows.iter().rposition(|row| *row != 0) else {
        return Vec::new();
    };
    rows[..=last]
        .iter()
        .rev()
        .map(|row| {
            (0..10)
                .map(|x| if row & (1 << x) == 0 { '_' } else { 'X' })
                .collect()
        })
        .collect()
}

pub(super) fn rotation_name(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::North => "north",
        Rotation::East => "east",
        Rotation::South => "south",
        Rotation::West => "west",
    }
}

pub(super) fn spin_name(spin: SpinType) -> &'static str {
    match spin {
        SpinType::NoSpin => "none",
        SpinType::Mini => "mini",
        SpinType::Full => "full",
    }
}
