//! Best-effort secret redaction for transcript text. Labeled rules run
//! first; entropy fallbacks run last and only fire next to a credential
//! keyword.

mod rules;

use regex::NoExpand;

use super::model::{AnalyzedSession, ContextEdit, ParsedSession, SubagentRef, TimelineEvent};
use rules::{Action, RULES};

/// Redact secret-looking substrings from one string.
pub fn redact_text(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    RULES
        .iter()
        .fold(s.to_owned(), |out, rule| match rule.action {
            Action::Static(to) => rule.re.replace_all(&out, NoExpand(to)).into_owned(),
            Action::Dynamic(f) => f(&rule.re, &out),
        })
}

fn redact_if_set(s: &mut String) {
    if !s.is_empty() {
        *s = redact_text(s);
    }
}

fn redact_event(e: &mut TimelineEvent) {
    redact_if_set(&mut e.title);
    redact_if_set(&mut e.preview);
    redact_if_set(&mut e.body);
    if let Some(ContextEdit::Replaced {
        original: Some(o), ..
    }) = &mut e.context_edit
    {
        redact_if_set(o);
    }
}

fn redact_subagent(s: &mut SubagentRef) {
    s.description.iter_mut().for_each(redact_if_set);
    s.workflow_phase.iter_mut().for_each(redact_if_set);
}

/// Redact every transcript-derived string of a parsed session in place.
pub fn redact_parsed(p: &mut ParsedSession) {
    p.title.iter_mut().for_each(redact_if_set);
    p.events.iter_mut().for_each(redact_event);
    p.subagents.iter_mut().for_each(redact_subagent);
}

/// Redact an analyzed session in place.
pub fn redact_session(a: &mut AnalyzedSession) {
    redact_parsed(&mut a.parsed);
}

#[cfg(test)]
mod tests {
    use super::rules::ascii_ci;
    use super::*;

    #[test]
    fn labeled_rules() {
        let key = format!("sk-ant-api03-{}", "A".repeat(40));
        assert_eq!(
            redact_text(&format!("k={key}")),
            "k=[REDACTED:anthropic-key]"
        );
        assert_eq!(
            redact_text("Authorization: Bearer abcdefghijklmnopqrstuvwxyz"),
            "Authorization: Bearer [REDACTED:bearer-token]"
        );
        assert_eq!(
            redact_text("postgres://user:hunter2@db/x"),
            "postgres://user:[REDACTED]@db/x"
        );
        assert_eq!(
            redact_text("sk-ant-short sk-abcdefghijklmnopqrstuv"),
            "sk-ant-short [REDACTED:openai-key]"
        );
        assert_eq!(
            redact_text("password = \"s3cretvalue\""),
            "password = [REDACTED:credential]"
        );
        assert_eq!(redact_text("password=changeme"), "password=changeme");
        assert_eq!(redact_text("ſecret=abcdefgh"), "ſecret=abcdefgh");
    }

    #[test]
    fn env_lines() {
        assert_eq!(
            redact_text("export API_TOKEN=abc123xyz\r\nAPI_URL=x\u{2028}DB_PASSWORD='hunter22'"),
            "export API_TOKEN=[REDACTED:credential]\r\nAPI_URL=x\u{2028}DB_PASSWORD=[REDACTED:credential]"
        );
        assert_eq!(
            redact_text("  export DB_PASS=hunter22x\nPRIVATE_URL=abcdefgh"),
            "DB_PASS=[REDACTED:env-secret]\nPRIVATE_URL=abcdefgh"
        );
        assert_eq!(
            redact_text("x\r\nAWS_CREDS='aaa'"),
            "x\r\nAWS_CREDS=[REDACTED:env-secret]"
        );
        assert_eq!(redact_text("SECRET_KEY=${SECRET}"), "SECRET_KEY=${SECRET}");
    }

    #[test]
    fn entropy_fallbacks() {
        let b64 = "Zx9Qw3Er5Ty7Ui9Op1As3Df5Gh7Jk9Lz2";
        assert_eq!(
            redact_text(&format!("my secret is {b64}")),
            "my secret is [REDACTED:high-entropy]"
        );
        assert_eq!(redact_text(&format!("plain {b64}")), format!("plain {b64}"));
        let hex = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            redact_text(&format!("webhook {hex}")),
            "webhook [REDACTED:high-entropy]"
        );
    }

    #[test]
    fn case_folding_is_ascii() {
        assert_eq!(ascii_ci(r"(?:ab)[a-z]\bX"), r"(?:[aA][bB])[a-z]\b[xX]");
        assert_eq!(ascii_ci(r"(?-u:\b)a"), r"(?-u:\b)[aA]");
    }
}
