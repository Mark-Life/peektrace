//! Session marks: model provider logos, harness and launcher monograms, and
//! the worktree marker. Logos are the models.dev SVGs in
//! `assets/logos/providers`, drawn in the theme's text colour.

use peektrace_protocol::AgentId;
use peektrace_protocol::wire::sessions::{Launcher, ModelProvider, Worktree};
use rune::{Color, El, Icon, icon, row, text};

use crate::model::list::launcher_help;
use crate::model::marks::{
    harness_mono, harness_name, launcher_mono, provider_help, provider_name,
};
use crate::theme::{Theme, agent_color, ty};

/// The side of a logo and of a monogram's box.
pub const MARK: f32 = 16.0;
const LOGO: f32 = 14.0;

macro_rules! logo {
    ($id:literal, $view:literal) => {
        Icon {
            name: concat!("provider-", $id),
            view: $view,
            body: include_str!(concat!("../../../../assets/logos/providers/", $id, ".svg")),
        }
    };
}

/// Every provider logo, by `ModelProvider::id`.
pub static LOGOS: [(&str, Icon); 13] = [
    ("alibaba", logo!("alibaba", 40.0)),
    ("anthropic", logo!("anthropic", 40.0)),
    ("cohere", logo!("cohere", 24.0)),
    ("deepseek", logo!("deepseek", 40.0)),
    ("google", logo!("google", 40.0)),
    ("meta", logo!("meta", 40.0)),
    ("minimax", logo!("minimax", 40.0)),
    ("mistral", logo!("mistral", 40.0)),
    ("moonshotai", logo!("moonshotai", 40.0)),
    ("nvidia", logo!("nvidia", 40.0)),
    ("openai", logo!("openai", 40.0)),
    ("xai", logo!("xai", 40.0)),
    ("zai", logo!("zai", 40.0)),
];

/// The worktree marker: a forked branch.
pub static WORKTREE: Icon = Icon {
    name: "worktree",
    view: 24.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9"/><path d="M12 12v3"/></g>"#,
};

/// The logo of a provider id, when one is committed.
pub fn provider_logo(id: &str) -> Option<&'static Icon> {
    LOGOS.iter().find(|(k, _)| *k == id).map(|(_, i)| i)
}

/// A small boxed monogram in `color`, named `name` for assistive tech.
fn mono(text_: &str, name: &str, color: Color, t: &Theme) -> El {
    row()
        .h(MARK)
        .min_w(MARK)
        .px(3.0)
        .radius(4.0)
        .center()
        .border(1.0, color.alpha(0.5))
        .fill(color.alpha(if t.dark { 0.14 } else { 0.08 }))
        .child(text(text_.to_owned()).font(ty::TINY).color(color))
        .role("img")
        .label(name.to_owned())
}

pub fn harness(a: AgentId, t: &Theme) -> El {
    mono(harness_mono(a), harness_name(a), agent_color(a), t).help(harness_name(a))
}

pub fn launcher(l: &Launcher, t: &Theme) -> El {
    mono(&launcher_mono(&l.name), &l.name, t.muted_fg, t).help(launcher_help(l))
}

/// The provider's logo, or a `?` for an unknown one; `None` when the
/// session names no model.
pub fn provider(
    p: Option<&ModelProvider>,
    model: Option<&str>,
    models: &[String],
    t: &Theme,
) -> Option<El> {
    let help = provider_help(p, model, models)?;
    let name = provider_name(p);
    let mark = match p.and_then(|p| provider_logo(&p.id)) {
        Some(i) => row()
            .size(MARK, MARK)
            .center()
            .child(icon(i, LOGO).color(t.fg))
            .role("img")
            .label(name.to_owned()),
        None => mono("?", name, t.muted_fg, t),
    };
    Some(mark.help(help))
}

/// Launcher (when present), harness and provider marks, in that order.
pub fn marks(
    launcher_: Option<&Launcher>,
    agent: Option<AgentId>,
    p: Option<&ModelProvider>,
    model: Option<&str>,
    models: &[String],
    t: &Theme,
) -> Vec<El> {
    [
        launcher_.map(|l| launcher(l, t)),
        agent.map(|a| harness(a, t)),
        provider(p, model, models, t),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The worktree marker: an icon whose tooltip gives the path and parent.
pub fn worktree(w: &Worktree, l: Option<&Launcher>, t: &Theme) -> El {
    row()
        .size(12.0, 12.0)
        .center()
        .child(icon(&WORKTREE, 12.0).color(t.muted_fg))
        .role("img")
        .label("Worktree")
        .help(crate::model::marks::worktree_help(w, l))
}

#[cfg(test)]
mod tests {
    use super::LOGOS;

    #[test]
    fn every_logo_view_matches_its_view_box() {
        for (id, i) in &LOGOS {
            let want = format!("viewBox=\"0 0 {0} {0}\"", i.view);
            assert!(i.body.contains(&want), "{id}: {want}");
            assert!(!i.body.contains("fill=\"#"), "{id} names its own colour");
        }
    }
}
