//! Query, path and body readers. Each failure is a 400 `BadRequestError`.

use axum::extract::Path;
use axum::extract::rejection::PathRejection;
use peektrace_core::Agent;
use peektrace_core::js::num;
use peektrace_core::js::value::{JsObject, JsValue};
use peektrace_protocol::ApiError;

use super::bad_request;

pub type Query = [(String, String)];

/// Every query param, decoded, in order.
pub fn query_pairs(raw: Option<&str>) -> Vec<(String, String)> {
    raw.map(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .into_owned()
            .collect()
    })
    .unwrap_or_default()
}

/// The first value of `key`.
pub fn param<'a>(q: &'a Query, key: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

pub fn redact_param(q: &Query) -> Result<bool, ApiError> {
    bool_param(q, "redact", true)
}

/// `true` or `false`, else `default` when absent.
pub fn bool_param(q: &Query, key: &str, default: bool) -> Result<bool, ApiError> {
    match param(q, key) {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(v) => Err(bad_request(format!(
            "{key} must be true or false, got {v:?}"
        ))),
    }
}

/// `Number(v)` when finite.
pub fn number_param(q: &Query, key: &str) -> Result<Option<f64>, ApiError> {
    param(q, key)
        .map(|v| {
            let n = num::parse_number(v);
            if v.is_empty() || !n.is_finite() {
                Err(bad_request(format!(
                    "{key} must be a finite number, got {v:?}"
                )))
            } else {
                Ok(n)
            }
        })
        .transpose()
}

/// An integer `Number(v)` in `range`.
pub fn int_param(
    q: &Query,
    key: &str,
    range: std::ops::RangeInclusive<f64>,
) -> Result<Option<f64>, ApiError> {
    match number_param(q, key)? {
        Some(n) if n.fract() != 0.0 || !range.contains(&n) => Err(bad_request(format!(
            "{key} must be a whole number from {} to {}, got {}",
            num::to_js_string(*range.start()),
            num::to_js_string(*range.end()),
            num::to_js_string(n)
        ))),
        other => Ok(other),
    }
}

fn agent_of(raw: &str) -> Result<Agent, ApiError> {
    Agent::parse(raw).ok_or_else(|| bad_request(format!("unknown agent {raw:?}")))
}

/// One optional `agent`; empty means every agent.
pub fn agent_param(q: &Query) -> Result<Option<Agent>, ApiError> {
    param(q, "agent")
        .filter(|a| !a.is_empty())
        .map(agent_of)
        .transpose()
}

/// Every `agent` value, empty ones skipped; none means every agent.
pub fn agents_param(q: &Query) -> Result<Option<Vec<Agent>>, ApiError> {
    let agents = q
        .iter()
        .filter(|(k, v)| k == "agent" && !v.is_empty())
        .map(|(_, v)| agent_of(v))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((!agents.is_empty()).then_some(agents))
}

/// A decoded path segment; a bad one (invalid UTF-8) is a 400.
pub fn segment<T>(p: Result<Path<T>, PathRejection>) -> Result<T, ApiError> {
    p.map(|Path(v)| v)
        .map_err(|e| bad_request(format!("bad path: {}", e.body_text())))
}

/// A JSON object body; an empty body reads as `{}`.
pub fn object_body(body: &[u8], empty_ok: bool) -> Result<JsObject, ApiError> {
    let text = std::str::from_utf8(body).map_err(|_| bad_request("body is not UTF-8"))?;
    if empty_ok && text.trim().is_empty() {
        return Ok(JsObject::default());
    }
    match JsValue::parse(text) {
        Some(JsValue::Obj(o)) => Ok(o),
        Some(_) => Err(bad_request("body must be a JSON object")),
        None => Err(bad_request("body is not JSON")),
    }
}

fn wrong(key: &str, kind: &str) -> ApiError {
    bad_request(format!("{key} must be {kind}"))
}

pub fn opt_str(o: &JsObject, key: &str) -> Result<Option<String>, ApiError> {
    match o.get(key) {
        None => Ok(None),
        Some(JsValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(wrong(key, "a string")),
    }
}

pub fn req_str(o: &JsObject, key: &str) -> Result<String, ApiError> {
    opt_str(o, key)?.ok_or_else(|| bad_request(format!("body needs {key}")))
}

pub fn opt_bool(o: &JsObject, key: &str) -> Result<Option<bool>, ApiError> {
    match o.get(key) {
        None => Ok(None),
        Some(JsValue::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(wrong(key, "a boolean")),
    }
}

pub fn opt_num(o: &JsObject, key: &str) -> Result<Option<f64>, ApiError> {
    match o.get(key) {
        None => Ok(None),
        Some(JsValue::Num(n)) => Ok(Some(*n)),
        Some(_) => Err(wrong(key, "a number")),
    }
}

pub fn opt_obj<'a>(o: &'a JsObject, key: &str) -> Result<Option<&'a JsObject>, ApiError> {
    match o.get(key) {
        None => Ok(None),
        Some(JsValue::Obj(v)) => Ok(Some(v)),
        Some(_) => Err(wrong(key, "an object")),
    }
}

/// An optional array of agent ids.
pub fn opt_agents(o: &JsObject, key: &str) -> Result<Option<Vec<Agent>>, ApiError> {
    match o.get(key) {
        None => Ok(None),
        Some(JsValue::Arr(items)) => items
            .iter()
            .map(|v| match v {
                JsValue::Str(s) => agent_of(s),
                _ => Err(wrong(key, "an array of agent ids")),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(wrong(key, "an array of agent ids")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn agents_repeat_and_skip_empty() {
        let got = agents_param(&q(&[("agent", "codex"), ("agent", ""), ("agent", "pi")]));
        assert_eq!(got, Ok(Some(vec![Agent::Codex, Agent::Pi])));
        assert_eq!(agents_param(&q(&[("agent", "")])), Ok(None));
        assert!(agents_param(&q(&[("agent", "all")])).is_err());
    }

    #[test]
    fn int_param_wants_a_whole_number_in_range() {
        let r = 1.0..=36_500.0;
        assert_eq!(
            int_param(&q(&[("days", "7")]), "days", r.clone()),
            Ok(Some(7.0))
        );
        assert!(int_param(&q(&[("days", "0")]), "days", r.clone()).is_err());
        assert!(int_param(&q(&[("days", "1.5")]), "days", r).is_err());
    }

    #[test]
    fn bodies_are_objects() {
        assert!(object_body(b"", true).is_ok_and(|o| o.is_empty()));
        assert!(object_body(b"", false).is_err());
        assert!(object_body(b"[1]", true).is_err());
        let o = object_body(br#"{"force":true,"agents":["claude"]}"#, false).unwrap_or_default();
        assert_eq!(opt_bool(&o, "force"), Ok(Some(true)));
        assert_eq!(opt_agents(&o, "agents"), Ok(Some(vec![Agent::Claude])));
        assert!(opt_str(&o, "force").is_err());
    }
}
