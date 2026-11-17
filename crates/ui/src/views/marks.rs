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
const MARK: f32 = 16.0;
const LOGO: f32 = 14.0;

/// The width an SVG's `viewBox="0 0 <w> <h>"` gives, read at compile time
/// so a logo's box needs no hand-copied number. Whole numbers only.
const fn view_of(svg: &str) -> f32 {
    const KEY: &[u8] = b"viewBox=\"";
    let b = svg.as_bytes();
    let mut i = 0;
    'find: while i + KEY.len() <= b.len() {
        let mut k = 0;
        while k < KEY.len() {
            if b[i + k] != KEY[k] {
                i += 1;
                continue 'find;
            }
            k += 1;
        }
        // Skip `<min-x> <min-y> ` to the width.
        let mut at = i + KEY.len();
        let mut gaps = 0;
        while gaps < 2 {
            if b[at] == b' ' {
                gaps += 1;
            }
            at += 1;
        }
        let mut w = 0.0;
        while b[at].is_ascii_digit() {
            w = w * 10.0 + (b[at] - b'0') as f32;
            at += 1;
        }
        return w;
    }
    panic!("svg has no viewBox");
}

macro_rules! logo {
    ($id:literal) => {{
        const BODY: &str =
            include_str!(concat!("../../../../assets/logos/providers/", $id, ".svg"));
        Icon {
            name: concat!("provider-", $id),
            view: view_of(BODY),
            body: BODY,
        }
    }};
}

/// Every provider logo, by `ModelProvider::id`.
pub static LOGOS: [(&str, Icon); 13] = [
    ("alibaba", logo!("alibaba")),
    ("anthropic", logo!("anthropic")),
    ("cohere", logo!("cohere")),
    ("deepseek", logo!("deepseek")),
    ("google", logo!("google")),
    ("meta", logo!("meta")),
    ("minimax", logo!("minimax")),
    ("mistral", logo!("mistral")),
    ("moonshotai", logo!("moonshotai")),
    ("nvidia", logo!("nvidia")),
    ("openai", logo!("openai")),
    ("xai", logo!("xai")),
    ("zai", logo!("zai")),
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

    #[test]
    fn every_maker_has_a_logo() {
        for id in peektrace_core::model_provider::MAKERS {
            assert!(LOGOS.iter().any(|(k, _)| *k == id), "{id} has no logo");
        }
    }
}
