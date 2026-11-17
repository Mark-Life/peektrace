//! What a session card shows about its source and place: the launcher,
//! harness and model-maker marks, the project label, and the branch.

use peektrace_core::model::{Launcher, SessionHeader};
use peektrace_core::model_provider::model_provider;

use crate::draw::{Seg, seg};
use crate::fmt::{clip, sanitize, take_width, width};
use crate::theme::{self, agent_color, agent_glyph, fg, provider_glyph};

/// Marks the worktree row's project.
pub const WORKTREE: &str = "⎇";
/// Separator between the project and the branch.
pub const SEP: &str = " · ";
/// The least columns a shrunk label keeps before it is dropped.
const MIN_LABEL: usize = 6;

/// A two-cell launcher mark: `T3` for t3code, else the name's first two
/// letters.
fn launcher_glyph(l: &Launcher) -> String {
    if l.name.eq_ignore_ascii_case("t3code") {
        return "T3".to_owned();
    }
    let mark: String = l
        .name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .collect();
    let mut chars = mark.chars();
    let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
    let mark = first.unwrap_or_default() + chars.as_str();
    take_width(&mark, 2).to_owned()
}

/// Launcher (when present), harness, then model maker: at most 8 cells.
pub fn glyphs(h: &SessionHeader) -> Vec<Seg> {
    let mut out = Vec::new();
    if let Some(l) = &h.launcher {
        out.push(seg(format!("{} ", launcher_glyph(l)), fg(theme::TEXT_DIM)));
    }
    out.push(seg(agent_glyph(h.agent), fg(agent_color(h.agent))));
    if let Some(m) = h.model.as_deref().filter(|m| !m.is_empty()) {
        let (g, c) = model_provider(m).map_or(("?", theme::TEXT_FAINT), |p| provider_glyph(p.id));
        out.push(seg(format!(" {g}"), fg(c)));
    }
    out
}

/// The last folder of a path.
pub fn leaf(path: &str) -> Option<&str> {
    path.split(['/', '\\']).rfind(|s| !s.is_empty())
}

/// The main checkout's name for a worktree row, else the session folder,
/// else the agent's project slug.
pub fn project_label(h: &SessionHeader) -> String {
    let label = h.worktree.as_ref().map_or_else(
        || {
            h.cwd
                .as_deref()
                .and_then(leaf)
                .unwrap_or(h.project.as_str())
        },
        |w| w.project.as_str(),
    );
    sanitize(label)
}

pub fn visible(branch: Option<&str>) -> Option<&str> {
    branch.filter(|b| !b.is_empty() && *b != "HEAD")
}

/// The transcript's branch, else the worktree's; never the worktree folder.
pub fn branch(h: &SessionHeader) -> Option<String> {
    visible(h.git_branch.as_deref())
        .or_else(|| visible(h.worktree.as_ref().and_then(|w| w.branch.as_deref())))
        .map(sanitize)
}

/// Project and branch fitted to `room` columns as `project · branch`. The
/// branch shrinks first, then the project; the branch drops when neither
/// keeps [`MIN_LABEL`] columns.
pub fn fit_place(project: &str, branch: Option<&str>, room: usize) -> (String, Option<String>) {
    let wp = width(project);
    let sep = width(SEP);
    let Some(b) = branch else {
        return (clip(project, room), None);
    };
    let wb = width(b);
    if wp + sep + wb <= room {
        return (project.to_owned(), Some(b.to_owned()));
    }
    let keep_b = MIN_LABEL.min(wb);
    let b_room = room.saturating_sub(wp + sep);
    if b_room >= keep_b {
        return (project.to_owned(), Some(clip(b, b_room)));
    }
    let p_room = room.saturating_sub(sep + keep_b);
    if p_room >= MIN_LABEL.min(wp) {
        return (clip(project, p_room), Some(clip(b, keep_b)));
    }
    (clip(project, room), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_shrinks_the_branch_first_then_the_project() {
        assert_eq!(
            fit_place("repo", Some("main"), 20),
            ("repo".into(), Some("main".into()))
        );
        assert_eq!(
            fit_place("repo", Some("feature/very-long-branch"), 16),
            ("repo".into(), Some("feature/…".into()))
        );
        assert_eq!(
            fit_place("a-very-long-project-name", Some("feature/x"), 16),
            ("a-very…".into(), Some("featu…".into()))
        );
        assert_eq!(
            fit_place("project", Some("branch"), 8),
            ("project".into(), None)
        );
        assert_eq!(
            fit_place("日本語のプロジェクト", None, 7),
            ("日本語…".into(), None)
        );
    }

    #[test]
    fn launcher_marks_are_two_cells() {
        let l = |name: &str| Launcher {
            name: name.to_owned(),
            branch: None,
            worktree_path: None,
            parent_title: None,
        };
        assert_eq!(launcher_glyph(&l("t3code")), "T3");
        assert_eq!(launcher_glyph(&l("conductor")), "Co");
        assert_eq!(launcher_glyph(&l("日本")), "日");
    }
}
