//! `GET /api/sessions/search`.

use axum::extract::{RawQuery, State};
use axum::http::Method;
use peektrace_core::js::num;
use peektrace_core::js::text;
use peektrace_core::js::time::{LocalZone, parse_date};
use peektrace_core::search::{
    DEFAULT_LIMIT, SearchEnv, SearchRequest, result_json, search_sessions,
};
use peektrace_protocol::ApiError;
use tracing::{Instrument as _, Span};

use super::params::{Query, agent_param, param, query_pairs, redact_param};
use super::{Res, Shared, bad_request, blocking, finish, json};

/// `limit`: an integer `>= 0`; 0 means every match.
fn limit_of(q: &Query) -> Result<usize, ApiError> {
    let Some(raw) = param(q, "limit") else {
        return Ok(DEFAULT_LIMIT);
    };
    let n = num::parse_number(raw);
    if raw.is_empty() || !num::is_safe_integer(n) || n < 0.0 {
        return Err(bad_request(format!(
            "limit must be a non-negative integer, got {raw:?}"
        )));
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a safe non-negative integer"
    )]
    Ok(n as usize)
}

fn request(q: &Query, zone: &dyn LocalZone) -> Result<SearchRequest, ApiError> {
    let text = param(q, "q").unwrap_or_default();
    if text::trim(text).is_empty() {
        return Err(bad_request("q must not be blank"));
    }
    let since_ms = param(q, "since")
        .map(|s| {
            parse_date(s, zone)
                .ok_or_else(|| bad_request(format!("since must be an ISO time, got {s:?}")))
        })
        .transpose()?;
    #[expect(clippy::cast_precision_loss, reason = "a JS date in ms")]
    let since_ms = since_ms.map(|ms| ms as f64);
    Ok(SearchRequest {
        q: text.to_owned(),
        agent: agent_param(q)?,
        project: param(q, "project").map(str::to_owned),
        since_ms,
        limit: limit_of(q)?,
        redact: redact_param(q)?,
    })
}

pub async fn search(State(state): Shared, method: Method, RawQuery(raw): RawQuery) -> Res {
    let span = rpc_span!("sessions.search", method, "/api/sessions/search");
    async move {
        let q = query_pairs(raw.as_deref());
        let result = async {
            let req = request(&q, state.zone.as_ref())?;
            let current = Span::current();
            let body = blocking(move || {
                let _entered = current.enter();
                let home = (!state.user_home.is_empty()).then_some(state.user_home.as_str());
                let env = SearchEnv {
                    roots: &state.roots,
                    home,
                    local: state.zone.as_ref(),
                    headers: Some(&state.headers),
                };
                Ok::<_, ApiError>(result_json(&search_sessions(env, &req).result, false))
            })
            .await?;
            Ok(json(body))
        }
        .await;
        finish(result)
    }
    .instrument(span)
    .await
}
