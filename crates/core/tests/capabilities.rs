//! The static support matrix (`capabilities.test.ts`).

use peektrace_core::capabilities::{SupportLevel, list_capabilities};

#[test]
fn rows_in_matrix_order() {
    let ids: Vec<_> = list_capabilities().iter().map(|c| c.id).collect();
    assert_eq!(
        ids,
        [
            "session.view",
            "session.debug-context",
            "memory.view",
            "memory.crud",
            "mcp.dashboard",
            "skills.browser",
            "file-history.diff"
        ]
    );
}

#[test]
fn memory_is_claude_only() {
    let crud = list_capabilities()
        .iter()
        .find(|c| c.id == "memory.crud")
        .expect("memory.crud");
    assert_eq!(crud.per_agent[0].level, SupportLevel::Supported);
    assert_eq!(
        crud.per_agent[0].note,
        Some("Claude markdown memories only")
    );
    assert!(
        crud.per_agent[1..]
            .iter()
            .all(|s| s.level == SupportLevel::Planned && s.note.is_none())
    );
}

#[test]
fn session_view_supported_everywhere() {
    let view = &list_capabilities()[0];
    assert!(
        view.per_agent
            .iter()
            .all(|s| s.level == SupportLevel::Supported)
    );
    let debug = &list_capabilities()[1];
    assert_eq!(debug.per_agent[3].level, SupportLevel::Partial);
    assert!(
        debug.per_agent[3]
            .note
            .is_some_and(|n| n.contains("inferred"))
    );
}
