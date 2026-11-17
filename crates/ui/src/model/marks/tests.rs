use peektrace_protocol::AgentId;
use peektrace_protocol::wire::sessions::{Launcher, ModelProvider, Worktree};

use super::*;

fn nvidia() -> ModelProvider {
    ModelProvider {
        id: "nvidia".into(),
        name: "NVIDIA".into(),
    }
}

#[test]
fn names_every_harness_with_a_short_monogram() {
    let marks: Vec<_> = AgentId::ALL
        .into_iter()
        .map(|a| (harness_mono(a), harness_name(a)))
        .collect();
    assert_eq!(
        marks,
        [
            ("CC", "Claude Code"),
            ("Cx", "Codex"),
            ("Pi", "Pi"),
            ("OC", "OpenCode")
        ]
    );
    assert_eq!(agent_of_provider("claude-code"), Some(AgentId::Claude));
    assert_eq!(agent_of_provider("opencode"), Some(AgentId::Opencode));
    assert_eq!(agent_of_provider("other"), None);
}

#[test]
fn shortens_a_launcher_to_two_letters() {
    assert_eq!(launcher_mono("t3code"), "T3");
    assert_eq!(launcher_mono("conductor"), "Co");
    assert_eq!(launcher_mono("--"), "?");
}

#[test]
fn names_the_provider_and_the_exact_model() {
    let p = nvidia();
    assert_eq!(
        provider_help(Some(&p), Some("nvidia/GLM-5.3-Flash-NVFP4"), &[]).as_deref(),
        Some("NVIDIA · nvidia/GLM-5.3-Flash-NVFP4")
    );
    assert_eq!(
        provider_help(None, Some("mystery-1"), &[]).as_deref(),
        Some("Unknown provider · mystery-1")
    );
    assert_eq!(provider_help(None, None, &[]), None);
    assert_eq!(provider_name(None), "Unknown provider");
}

#[test]
fn lists_every_model_under_the_main_one() {
    let p = nvidia();
    let models = vec!["kimi-k3".to_owned(), "nvidia/nemotron".to_owned()];
    assert_eq!(
        provider_help(Some(&p), Some("nvidia/nemotron"), &models).as_deref(),
        Some("NVIDIA · nvidia/nemotron\nkimi-k3")
    );
    assert_eq!(
        provider_help(Some(&p), None, &models).as_deref(),
        Some("NVIDIA · kimi-k3\nnvidia/nemotron")
    );
}

#[test]
fn gives_the_worktree_path_and_the_parent_thread() {
    let w = Worktree {
        project: "peephole".into(),
        project_root: Some("/code/peephole".into()),
        path: "/home/u/.t3/worktrees/peephole/t3-1".into(),
        branch: Some("feat/x".into()),
    };
    assert_eq!(
        worktree_help(&w, None),
        "Worktree: /home/u/.t3/worktrees/peephole/t3-1"
    );
    let l = Launcher {
        name: "t3code".into(),
        branch: None,
        worktree_path: None,
        parent_title: Some("Plan the release".into()),
    };
    assert_eq!(
        worktree_help(&w, Some(&l)),
        "Worktree: /home/u/.t3/worktrees/peephole/t3-1\nStarted by: Plan the release"
    );
}
