use super::*;

fn id(model: &str) -> Option<&'static str> {
    model_provider(model).map(|m| m.id)
}

#[test]
fn maps_ids_by_org_exact_id_and_family() {
    let cases = [
        // Org prefixes, including host ids and cloud ids.
        ("nvidia/GLM-5.3-Flash-NVFP4", Some("nvidia")),
        ("zai-org/GLM-5.3-Flash", Some("zai")),
        ("moonshotai/Kimi-K3", Some("moonshotai")),
        ("meta-llama/Llama-4-Scout", Some("meta")),
        ("qwen/qwen3-coder-30b", Some("alibaba")),
        ("deepseek-ai/DeepSeek-V4", Some("deepseek")),
        ("mistralai/Devstral-Small", Some("mistral")),
        ("x-ai/grok-4.6", Some("xai")),
        ("anthropic/claude-opus-5-5", Some("anthropic")),
        ("us.anthropic.claude-opus-4-7", Some("anthropic")),
        ("anthropic.claude-fable-5-1", Some("anthropic")),
        // An unknown org: the model part decides.
        ("modal-kimi/kimi-k3", Some("moonshotai")),
        ("some-host/glm-5", Some("zai")),
        // Exact ids in a maker's list.
        ("muse-spark-1.3", Some("meta")),
        ("MiniMax-M2.7", Some("minimax")),
        ("kimi-k3", Some("moonshotai")),
        // Families.
        ("claude-opus-5-5", Some("anthropic")),
        ("claude-sonnet-9-20990101", Some("anthropic")),
        ("gpt-5.5", Some("openai")),
        ("gpt-5.3-codex", Some("openai")),
        ("o3", Some("openai")),
        ("o4-mini", Some("openai")),
        ("gemini-9-pro", Some("google")),
        ("gemma-4", Some("google")),
        ("grok-9", Some("xai")),
        ("glm-9", Some("zai")),
        ("qwen9-max", Some("alibaba")),
        ("deepseek-v9", Some("deepseek")),
        ("codestral-2099", Some("mistral")),
        ("llama-5", Some("meta")),
        ("nemotron-ultra", Some("nvidia")),
        // Unknown.
        ("o1x", None),
        ("my-local-model", None),
        ("", None),
        ("unknown", None),
    ];
    for (model, want) in cases {
        assert_eq!(id(model), want, "{model}");
    }
}

#[test]
fn every_maker_has_a_name_and_a_logo() {
    for maker in MAKERS {
        let m = mark(maker).expect(maker);
        assert_ne!(m.name, "");
        let path = format!(
            "{}/../../assets/logos/providers/{maker}.svg",
            env!("CARGO_MANIFEST_DIR")
        );
        let svg = std::fs::read_to_string(&path).expect(&path);
        assert!(
            svg.contains("<svg") && svg.contains("currentColor"),
            "{maker}"
        );
    }
    assert_eq!(mark("nvidia").map(|m| m.name), Some("NVIDIA"));
}

#[test]
fn tooltips_name_the_provider_and_exact_model() {
    assert_eq!(
        provider_line("nvidia/GLM-5.3-Flash-NVFP4"),
        "NVIDIA · nvidia/GLM-5.3-Flash-NVFP4"
    );
    assert_eq!(provider_line("mine"), "Unknown provider · mine");
}

#[test]
fn snapshot_keeps_each_id_with_its_own_maker() {
    let api = r#"{
        "anthropic": {"name": "Anthropic", "models": {"claude-x": {}}},
        "openai": {"name": "OpenAI", "models": {"gpt-x": {}, "openai/gpt-oss": {}}},
        "google": {"name": "Google", "models": {}},
        "xai": {"name": "xAI", "models": {}},
        "zai": {"name": "Z.AI", "models": {"glm-x": {}}},
        "moonshotai": {"name": "Moonshot AI", "models": {"kimi-x": {}}},
        "alibaba": {"name": "Alibaba", "models": {"qwen-x": {}, "kimi-x": {}, "plain": {}}},
        "deepseek": {"name": "DeepSeek", "models": {}},
        "mistral": {"name": "Mistral", "models": {"zai-glm-x": {}, "plain": {}}},
        "minimax": {"name": "MiniMax (minimax.io)", "models": {"MiniMax-M9": {}}},
        "meta": {"name": "Meta", "models": {}},
        "nvidia": {"name": "Nvidia", "models": {"nvidia/x": {}}},
        "cohere": {"name": "Cohere", "models": {}}
    }"#;
    let text = providers_snapshot(api).expect("snapshot");
    let line = |id: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("{id}\t")))
            .map(str::to_owned)
    };
    assert_eq!(line("openai").as_deref(), Some("openai\tOpenAI\tgpt-x"));
    assert_eq!(
        line("alibaba").as_deref(),
        Some("alibaba\tAlibaba\tplain qwen-x")
    );
    assert_eq!(line("mistral").as_deref(), Some("mistral\tMistral\t"));
    assert_eq!(
        line("minimax").as_deref(),
        Some("minimax\tMiniMax\tminimax-m9")
    );
    assert_eq!(line("nvidia").as_deref(), Some("nvidia\tNVIDIA\t"));
    let parsed = parse(Box::leak(text.into_boxed_str()));
    assert_eq!(parsed.makers.len(), MAKERS.len());
    assert_eq!(parsed.exact.get("plain"), Some(&6));
    assert!(providers_snapshot("{}").is_err());
}

#[test]
fn committed_snapshot_lists_every_maker() {
    let ids: Vec<_> = TABLE.makers.iter().map(|m| m.id).collect();
    assert_eq!(ids, MAKERS);
}
