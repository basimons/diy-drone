//! Choosing what to ask for in one recording session.

use rand::{Rng, RngExt};

use crate::plan::{Plan, PromptCell, PromptKind};
use crate::store::Progress;

/// What the user ticked at start-up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub kinds: Vec<PromptKind>,
    pub conditions: Vec<String>,
    pub length: usize, // takes in the session
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueuedTake {
    pub cell: PromptCell,
    pub style: String,
}

/// Up to `selection.length` takes from the cells that still need some, each cell weighted by how many it still needs,
/// never more than it needs, and never the same prompt twice in a row while another prompt is left.
pub fn pick(
    plan: &Plan,
    progress: &Progress,
    is_removed: impl Fn(&PromptCell) -> bool,
    selection: &Selection,
    rng: &mut impl Rng,
) -> Vec<QueuedTake> {
    debug_assert!(!plan.styles().is_empty());
    let mut left: Vec<(&PromptCell, u32)> = plan
        .prompt_cells()
        .iter()
        .filter(|cell| return selection.kinds.contains(&cell.kind) && selection.conditions.contains(&cell.condition_id))
        .filter(|cell| return !is_removed(cell))
        .map(|cell| return (cell, cell.target.saturating_sub(progress.takes(cell))))
        .filter(|(_, left)| return *left > 0)
        .collect();

    let mut queue: Vec<QueuedTake> = Vec::new();
    while queue.len() < selection.length {
        let previous = queue.last().map(|take| return take.cell.prompt_id.as_str());
        let open = |i: &usize| return left[*i].1 > 0;
        let fresh: Vec<usize> = (0..left.len())
            .filter(open)
            .filter(|&i| return Some(left[i].0.prompt_id.as_str()) != previous)
            .collect();
        let candidates = if fresh.is_empty() {
            (0..left.len()).filter(open).collect()
        } else {
            fresh
        };
        let Some(chosen) = weighted(&candidates, |i| return left[i].1, rng) else {
            break;
        };
        left[chosen].1 -= 1;
        let style = plan.styles()[rng.random_range(0..plan.styles().len())].clone();
        queue.push(QueuedTake {
            cell: left[chosen].0.clone(),
            style,
        });
    }
    return queue;
}

fn weighted(candidates: &[usize], weight: impl Fn(usize) -> u32, rng: &mut impl Rng) -> Option<usize> {
    let total: u32 = candidates.iter().map(|&i| return weight(i)).sum();
    if total == 0 {
        return None;
    }
    let mut ticket = rng.random_range(0..total);
    for &i in candidates {
        if ticket < weight(i) {
            return Some(i);
        }
        ticket -= weight(i);
    }
    unreachable!("ticket is below the total weight");
}
