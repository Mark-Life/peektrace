//! A best-effort context window by model id, for transcripts that do not
//! record it (Claude without a `[1m]` key, Pi). A low-confidence guess: the
//! caller drops it when a call is larger.

const K128: f64 = 128_000.0;
const K131: f64 = 131_072.0;
const K200: f64 = 200_000.0;
const K250: f64 = 250_000.0;
const K258: f64 = 258_400.0;
const M1: f64 = 1_000_000.0;

/// Substring and window for non-Claude models; the first hit in order wins,
/// so more specific families come first.
const WINDOWS: [(&str, f64); 9] = [
    ("gpt-5", K258),
    ("gpt-oss", K131),
    ("glm-5", K250),
    ("glm", K128),
    ("qwen3", K131),
    ("qwen", K131),
    ("gemma", K128),
    ("gemini", M1),
    ("llama", K128),
];

/// Claude families and the first `(major, minor)` with a 1M window.
const CLAUDE_1M_FROM: [(&str, (u32, u32)); 3] =
    [("opus", (4, 6)), ("sonnet", (4, 6)), ("haiku", (5, 0))];

/// A version part: 1-2 digits. Longer runs are dates (`20250514`).
fn version_part(s: &str) -> Option<u32> {
    (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// `(major, minor)` from the leading parts, as in `4-1-20250805` or
/// `4-20250514` (minor 0).
fn version(parts: &[&str]) -> Option<(u32, u32)> {
    let major = version_part(parts.first()?)?;
    Some((
        major,
        parts.get(1).and_then(|p| version_part(p)).unwrap_or(0),
    ))
}

/// A Claude id: 200K for opus/sonnet before 4.6, haiku before 5 and the
/// 1.x-3.x lines; 1M for later ones and any other Claude family. The
/// version may follow the family (`opus-4-1`) or lead it (`3-5-sonnet`).
fn claude_window(id: &str) -> f64 {
    let rest = id.split_once("claude-").map_or(id, |(_, r)| r);
    let parts: Vec<&str> = rest.split(['-', '.', '@', '[', ':']).collect();
    let family = parts.iter().enumerate().find_map(|(i, p)| {
        CLAUDE_1M_FROM
            .iter()
            .find(|(f, _)| f == p)
            .map(|(_, from)| (i, *from))
    });
    let Some((at, from)) = family else {
        let old = ["instant", "v"].iter().any(|p| rest.starts_with(p))
            || version(&parts).is_some_and(|(major, _)| major < 4);
        return if old { K200 } else { M1 };
    };
    match version(&parts[at + 1..]).or_else(|| version(&parts[..at])) {
        Some(v) if v < from => K200,
        _ => M1,
    }
}

/// The window for a model id, matched case-insensitively, or `None`. A
/// `[1m]` suffix means the 1M window.
pub fn window_for_model(model: Option<&str>) -> Option<f64> {
    let needle = model.filter(|m| !m.is_empty())?.to_lowercase();
    if needle.ends_with("[1m]") {
        return Some(M1);
    }
    if needle.contains("claude") {
        return Some(claude_window(&needle));
    }
    WINDOWS
        .iter()
        .find(|(key, _)| needle.contains(key))
        .map(|&(_, w)| w)
}

/// The window for a session with `models` and a largest call of `peak`
/// tokens: the widest guess among its models, else 1M. A call above the
/// guess proves it wrong, so 1M then too.
pub fn session_window<'a>(models: impl IntoIterator<Item = &'a str>, peak: f64) -> f64 {
    let guess = models
        .into_iter()
        .filter_map(|m| window_for_model(Some(m)))
        .reduce(f64::max)
        .unwrap_or(M1);
    if peak > guess { M1 } else { guess }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_hit_wins() {
        assert_eq!(window_for_model(Some("z-ai/glm-5.2")), Some(K250));
        assert_eq!(window_for_model(Some("glm-4.6")), Some(K128));
        assert_eq!(window_for_model(Some("gpt-5.5")), Some(K258));
        assert_eq!(window_for_model(Some("kimi")), None);
        assert_eq!(window_for_model(Some("")), None);
        assert_eq!(window_for_model(None), None);
    }

    #[test]
    fn claude_windows_by_family_and_version() {
        let w = |m: &str| window_for_model(Some(m));
        for small in [
            "claude-haiku-4-5",
            "claude-haiku-4-5-20251001",
            "claude-3-5-sonnet-20241022",
            "claude-3-opus-20240229",
            "claude-opus-4-20250514",
            "claude-opus-4-1-20250805",
            "claude-sonnet-4-5-20250929",
            "anthropic/claude-sonnet-4.5",
            "claude-opus-4-5@20251101",
            "us.anthropic.claude-opus-4-1-20250805-v1:0",
            "claude-4-sonnet",
            "claude-4.5-sonnet",
            "claude-3-haiku-20240307",
            "claude-2.1",
            "claude-instant-1.2",
            "anthropic.claude-v2:1",
        ] {
            assert_eq!(w(small), Some(K200), "{small}");
        }
        for large in [
            "claude-opus-4-6",
            "Claude-Opus-4-8",
            "claude-opus-5-5",
            "claude-sonnet-4-6",
            "claude-sonnet-5-5",
            "claude-haiku-5-5",
            "claude-fable-5-2",
            "claude-mythos-1",
            "us.anthropic.claude-opus-4-6-v1:0",
            "claude-4.6-sonnet",
            "claude-sonnet-4-5[1m]",
            "claude",
        ] {
            assert_eq!(w(large), Some(M1), "{large}");
        }
    }

    #[test]
    fn a_session_takes_its_widest_model_and_a_bigger_call_forces_1m() {
        assert_eq!(session_window(["claude-haiku-4-5"], 11_000.0), K200);
        assert_eq!(session_window(["claude-haiku-4-5"], 200_001.0), M1);
        assert_eq!(
            session_window(["claude-haiku-4-5", "claude-opus-5-5"], 0.0),
            M1
        );
        assert_eq!(session_window([], 0.0), M1);
    }
}
