use std::collections::BTreeSet;

use serde_json::json;
use tempfile::tempdir;
use zode_app_server_protocol::rpc::INVALID_PARAMS;
use zode_core::config::ZodeConfig;

use crate::config_write::{merge_patch, persist_patch, CONFIG_KEYS};

#[test]
fn whitelist_matches_every_serialized_zode_config_key() {
    let serialized = serde_json::to_value(ZodeConfig::default()).unwrap();
    let actual = serialized
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let declared = [
        "provider",
        "providers",
        "images",
        "theme",
        "currency",
        "language",
        "goal",
        "autoLoopMaxTurns",
        "effort",
        "showThinking",
        "showToolDetails",
        "mouseCapture",
        "autonomousOrchestration",
        "skillDiscipline",
        "openspecAwareness",
        "permissions",
        "sandbox",
        "maxOutputTokens",
        "maxIterations",
        "maxApiRetries",
        "autoUpdate",
        "contextWindow",
        "temperature",
        "promptCache",
        "plugins",
        "tools",
        "lsp",
        "openpencil",
        "browser",
        "noema",
        "compact",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    let expected = CONFIG_KEYS
        .iter()
        .map(|key| (*key).to_string())
        .collect::<BTreeSet<_>>();
    assert!(actual.is_subset(&expected));
    assert_eq!(expected, declared);
}

#[test]
fn unknown_patch_key_is_rejected_by_name() {
    let error = merge_patch(&ZodeConfig::default(), json!({"notAConfigKey": true})).unwrap_err();
    assert_eq!(error.code, INVALID_PARAMS);
    assert!(error.message.contains("notAConfigKey"));
}

#[test]
fn invalid_patch_value_reports_serde_error() {
    let error = merge_patch(&ZodeConfig::default(), json!({"maxIterations": "many"})).unwrap_err();
    assert_eq!(error.code, INVALID_PARAMS);
    assert!(error.message.contains("invalid type"));
}

#[test]
fn shallow_patch_replaces_a_top_level_value() {
    let config = merge_patch(&ZodeConfig::default(), json!({"theme": "dark"})).unwrap();
    assert_eq!(config.theme.as_deref(), Some("dark"));
}

#[test]
fn persist_patch_preserves_unknown_file_keys() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, r#"{"futureKey":{"keep":true},"theme":"light"}"#).unwrap();

    persist_patch(dir.path(), &json!({"theme": "dark"})).unwrap();

    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["futureKey"], json!({"keep": true}));
    assert_eq!(saved["theme"], "dark");
}
