use crate::header::{piece_table, Piece, ALL_ROTATIONS};

use super::{ShowcasePlacement, SHOWCASE_WIDTH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DiffCell {
    x: i16,
    y: i16,
    letter: char,
}

struct Diff {
    cells: Vec<DiffCell>,
}

pub(super) fn derive(
    parent: &[String],
    child: &[String],
) -> Option<(Vec<ShowcasePlacement>, Vec<String>)> {
    let diff = diff_pages(parent, child)?;
    let placements = tile_diff(&diff)?;
    let merged = merge_diff(parent, &diff)?;
    Some((placements, merged))
}

fn diff_pages(parent: &[String], child: &[String]) -> Option<Diff> {
    let mut cells = Vec::new();
    let height = parent.len().max(child.len());
    for y in 0..height {
        for x in 0..SHOWCASE_WIDTH {
            let before = cell_at(parent, x, y);
            let after = cell_at(child, x, y);
            if before != '_' {
                if after == '_' {
                    return None;
                }
                continue;
            }
            if after == '_' {
                continue;
            }
            if after == 'X' {
                return None;
            }
            cells.push(DiffCell {
                x: i16::try_from(x).ok()?,
                y: i16::try_from(y).ok()?,
                letter: after,
            });
        }
    }
    Some(Diff { cells })
}

fn cell_at(rows: &[String], x: usize, y: usize) -> char {
    rows.get(y)
        .and_then(|row| row.chars().nth(x))
        .unwrap_or('_')
}

fn tile_diff(diff: &Diff) -> Option<Vec<ShowcasePlacement>> {
    let mut placements = Vec::new();
    for (letter, component) in components(diff) {
        placements.extend(tile_component(letter, component)?);
    }
    Some(placements)
}

fn components(diff: &Diff) -> Vec<(char, Vec<DiffCell>)> {
    let mut seen = vec![false; diff.cells.len()];
    let mut components = Vec::new();

    for index in 0..diff.cells.len() {
        if seen[index] {
            continue;
        }
        let letter = diff.cells[index].letter;
        let mut stack = vec![index];
        let mut component = Vec::new();
        seen[index] = true;

        while let Some(current_index) = stack.pop() {
            let current = diff.cells[current_index];
            component.push(current);
            for (delta_x, delta_y) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let neighbor_x = current.x + delta_x;
                let neighbor_y = current.y + delta_y;
                let neighbor = diff.cells.iter().position(|cell| {
                    cell.x == neighbor_x && cell.y == neighbor_y && cell.letter == letter
                });
                if let Some(neighbor_index) =
                    neighbor.filter(|neighbor_index| !seen[*neighbor_index])
                {
                    seen[neighbor_index] = true;
                    stack.push(neighbor_index);
                }
            }
        }
        components.push((letter, component));
    }
    components
}

fn tile_component(letter: char, component: Vec<DiffCell>) -> Option<Vec<ShowcasePlacement>> {
    if !component.len().is_multiple_of(4) {
        return None;
    }
    if component.len() == 4 {
        return Some(vec![ShowcasePlacement {
            letter: letter.to_string(),
            cells: sorted_cells(&component)?,
        }]);
    }

    let shapes = piece_shapes(letter);
    let mut remaining = component;
    let mut placed = Vec::new();
    tile_remaining(letter, &shapes, &mut remaining, &mut placed).then_some(placed)
}

fn tile_remaining(
    letter: char,
    shapes: &[Vec<DiffCell>],
    remaining: &mut Vec<DiffCell>,
    placed: &mut Vec<ShowcasePlacement>,
) -> bool {
    let Some(anchor) = remaining
        .iter()
        .min_by_key(|cell| (cell.y, cell.x))
        .copied()
    else {
        return true;
    };
    for shape in shapes {
        for shape_anchor in shape {
            let cells = translated_cells(shape, *shape_anchor, anchor);
            if !cells.iter().all(|cell| remaining.contains(cell)) {
                continue;
            }
            let Some(placement_cells) = cells_to_u8(&cells) else {
                continue;
            };
            remaining.retain(|cell| !cells.contains(cell));
            placed.push(ShowcasePlacement {
                letter: letter.to_string(),
                cells: placement_cells,
            });
            if tile_remaining(letter, shapes, remaining, placed) {
                return true;
            }
            placed.pop();
            remaining.extend(cells);
        }
    }
    false
}

fn translated_cells(shape: &[DiffCell], shape_anchor: DiffCell, anchor: DiffCell) -> Vec<DiffCell> {
    shape
        .iter()
        .map(|cell| DiffCell {
            x: anchor.x + cell.x - shape_anchor.x,
            y: anchor.y + cell.y - shape_anchor.y,
            letter: anchor.letter,
        })
        .collect()
}

fn piece_shapes(letter: char) -> Vec<Vec<DiffCell>> {
    let Some(piece) = piece_for_letter(letter) else {
        return Vec::new();
    };
    let mut shapes = Vec::new();
    for rotation in ALL_ROTATIONS {
        let table = piece_table(piece, rotation);
        let mut cells = Vec::with_capacity(4);
        cells.push(DiffCell { x: 0, y: 0, letter });
        for index in 0..3 {
            let offset = table[index];
            cells.push(DiffCell {
                x: i16::from(offset.x),
                y: i16::from(offset.y),
                letter,
            });
        }
        normalize_cells(&mut cells);
        if !shapes.contains(&cells) {
            shapes.push(cells);
        }
    }
    shapes
}

fn piece_for_letter(letter: char) -> Option<Piece> {
    match letter {
        'I' => Some(Piece::I),
        'O' => Some(Piece::O),
        'T' => Some(Piece::T),
        'S' => Some(Piece::S),
        'Z' => Some(Piece::Z),
        'J' => Some(Piece::J),
        'L' => Some(Piece::L),
        _ => None,
    }
}

fn normalize_cells(cells: &mut [DiffCell]) {
    let mut min_x = 0;
    let mut min_y = 0;
    for cell in cells.iter() {
        min_x = min_x.min(cell.x);
        min_y = min_y.min(cell.y);
    }
    for cell in cells.iter_mut() {
        cell.x -= min_x;
        cell.y -= min_y;
    }
    cells.sort_by_key(|cell| (cell.y, cell.x));
}

fn sorted_cells(cells: &[DiffCell]) -> Option<Vec<[u8; 2]>> {
    let mut cells = cells.to_vec();
    cells.sort_by_key(|cell| (cell.y, cell.x));
    cells_to_u8(&cells)
}

fn cells_to_u8(cells: &[DiffCell]) -> Option<Vec<[u8; 2]>> {
    cells
        .iter()
        .map(|cell| Some([u8::try_from(cell.x).ok()?, u8::try_from(cell.y).ok()?]))
        .collect()
}

fn merge_diff(board: &[String], diff: &Diff) -> Option<Vec<String>> {
    let mut rows: Vec<Vec<char>> = board.iter().map(|row| row.chars().collect()).collect();
    for cell in &diff.cells {
        let y = usize::try_from(cell.y).ok()?;
        let x = usize::try_from(cell.x).ok()?;
        while rows.len() <= y {
            rows.push(vec!['_'; SHOWCASE_WIDTH]);
        }
        rows[y][x] = cell.letter;
    }
    Some(
        rows.into_iter()
            .map(|row| row.into_iter().collect())
            .collect(),
    )
}
