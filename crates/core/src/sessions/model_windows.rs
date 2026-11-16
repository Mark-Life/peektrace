//! A best-effort context window by model id, for agents whose transcripts do
//! not record it (Pi). A low-confidence guess: the caller drops it when a
//! call is larger.

const K128: f64 = 128_000.0;
const K131: f64 = 131_072.0;
const K200: f64 = 200_000.0;
const K250: f64 = 250_000.0;
const K258: f64 = 258_400.0;
const M1: f64 = 1_000_000.0;

/// Substring and window; the first hit in order wins, so more specific
/// families come first.
const WINDOWS: [(&str, f64); 13] = [
    ("claude-opus", K200),
    ("claude-sonnet", M1),
    ("claude-haiku", K200),
    ("claude", K200),
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

/// The window for a model id, matched case-insensitively, or `None`.
pub fn window_for_model(model: Option<&str>) -> Option<f64> {
    let needle = model.filter(|m| !m.is_empty())?.to_lowercase();
    WINDOWS
        .iter()
        .find(|(key, _)| needle.contains(key))
        .map(|&(_, w)| w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_hit_wins() {
        assert_eq!(window_for_model(Some("Claude-Opus-4-8")), Some(K200));
        assert_eq!(window_for_model(Some("z-ai/glm-5.2")), Some(K250));
        assert_eq!(window_for_model(Some("glm-4.6")), Some(K128));
        assert_eq!(window_for_model(Some("gpt-5.5")), Some(K258));
        assert_eq!(window_for_model(Some("kimi")), None);
        assert_eq!(window_for_model(Some("")), None);
        assert_eq!(window_for_model(None), None);
    }
}
