//! An opener version is the clears its path performs, in order, named as the
//! catalog names them. Shapes of one version are interchangeable for a build;
//! shapes of different versions never are.

use crate::openers::catalog::OpenerTreeNode;

/// The clears on the path from the root to `node`, in order. A clear the
/// catalog leaves unnamed is named by its line count, with ` PC` when the
/// board after it is empty. None when `node` or an ancestor is missing.
pub(super) fn clear_names(tree: &[OpenerTreeNode], node: u32) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut current = Some(node);
    while let Some(id) = current {
        let at = tree.iter().find(|candidate| candidate.id == id)?;
        let lines = at.clear_rows.as_ref().map_or(0, Vec::len);
        if lines > 0 {
            names.push(clear_name(at, lines));
        }
        current = at.parent;
    }
    names.reverse();
    Some(names)
}

fn clear_name(node: &OpenerTreeNode, lines: usize) -> String {
    if let Some(label) = node.est.as_ref().and_then(|est| est.clear.clone()) {
        return label;
    }
    let mut name = match lines {
        1 => "single",
        2 => "double",
        3 => "triple",
        _ => "quad",
    }
    .to_owned();
    if node
        .rows
        .iter()
        .all(|row| row.bytes().all(|cell| cell == b'_'))
    {
        name.push_str(" PC");
    }
    name
}

/// The versions of a tree: lettered leaves grouped by their clears, in tree
/// order. Mosaic's library groups them the same way.
#[cfg(test)]
pub(super) fn version_shapes(tree: &[OpenerTreeNode]) -> Vec<(Vec<String>, Vec<u32>)> {
    let mut versions: Vec<(Vec<String>, Vec<u32>)> = Vec::new();
    for node in tree {
        if node.grey || tree.iter().any(|child| child.parent == Some(node.id)) {
            continue;
        }
        let Some(names) = clear_names(tree, node.id) else {
            continue;
        };
        match versions.iter_mut().find(|(known, _)| *known == names) {
            Some((_, shapes)) => shapes.push(node.id),
            None => versions.push((names, vec![node.id])),
        }
    }
    versions
}
