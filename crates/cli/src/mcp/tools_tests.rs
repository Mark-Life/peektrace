use super::super::protocol::call_reply;
use super::super::run_call;
use super::*;

/// No corpus: an empty home that no test reads, since every case fails
/// or dies before a tool touches the disk.
fn shared() -> Shared {
    Shared::new(Context {
        roots_env: peektrace_core::roots::RootsEnv {
            home: "/nonexistent-peektrace-home".to_owned(),
            ..Default::default()
        },
        settings: None,
        cwd: None,
        now_ms: 0.0,
    })
}

fn args(text: &str) -> JsValue {
    JsValue::parse(text).unwrap_or(JsValue::Null)
}

/// TS `mcp-protocol.test.ts` "%s refuses redact:false", one per tool.
#[test]
fn every_tool_refuses_redact_false() {
    let s = shared();
    for tool in &TOOLS {
        let r = call_reply(
            &JsValue::Num(8.0),
            run_call(tool, &args(r#"{"redact":false}"#), &s),
        );
        let v = JsValue::parse(&r).unwrap_or(JsValue::Null);
        let result = v.get("result");
        assert_eq!(
            result.and_then(|r| r.get("isError")),
            Some(&JsValue::Bool(true)),
            "{}: {r}",
            tool.name
        );
        let text = result
            .and_then(|r| r.get("content"))
            .and_then(JsValue::as_arr)
            .and_then(|c| c.first().and_then(|b| b.get("text")))
            .and_then(JsValue::as_str)
            .unwrap_or("");
        assert!(text.contains("redact"), "{}: {text}", tool.name);
    }
}

/// TS `mcp-protocol.test.ts` "a defect becomes an isError result".
#[test]
fn a_defect_becomes_an_is_error_result() {
    let boom = Tool {
        name: "boom",
        description: "",
        fields: &[],
        run: |_, _| panic!("kaput"),
    };
    let outcome = run_call(&boom, &JsValue::Null, &shared());
    assert_eq!(
        call_reply(&JsValue::Num(9.0), outcome),
        r#"{"jsonrpc":"2.0","id":9,"result":{"content":[{"type":"text","text":"kaput"}],"isError":true}}"#
    );
}
