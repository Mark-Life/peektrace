//! Element-tree queries the view tests share.

#![allow(dead_code, reason = "each test binary uses part of it")]

use rune::ElementInfo;

/// Leaves under `key`'s element with no text: the skeleton blocks.
pub fn blank_leaves_under(els: &[ElementInfo], key: &str) -> usize {
    let Some(root) = els.iter().position(|e| e.key.as_deref() == Some(key)) else {
        return 0;
    };
    let under = |mut i: usize| {
        while let Some(p) = els[i].parent {
            if p == root {
                return true;
            }
            i = p;
        }
        false
    };
    (0..els.len())
        .filter(|&i| under(i))
        .filter(|&i| !els.iter().any(|e| e.parent == Some(i)))
        .filter(|&i| els[i].text.is_none())
        .count()
}

/// The first element keyed `key`.
pub fn by_key<'a>(els: &'a [ElementInfo], key: &str) -> Option<&'a ElementInfo> {
    els.iter().find(|e| e.key.as_deref() == Some(key))
}

/// True when `inner` lies inside `outer`, to half a point.
pub fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
    inner[0] >= outer[0] - 0.5
        && inner[1] >= outer[1] - 0.5
        && inner[0] + inner[2] <= outer[0] + outer[2] + 0.5
        && inner[1] + inner[3] <= outer[1] + outer[3] + 0.5
}
