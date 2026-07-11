use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::schema::{fixture_messages, protocol_schema};

fn json_files(dir: &Path) -> BTreeSet<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| path.file_name().unwrap().into())
        .collect()
}

#[test]
fn checked_in_fixtures_match_generated_fixtures() {
    let generated_dir = tempfile::tempdir().unwrap();
    for fixture in fixture_messages() {
        let path = generated_dir.path().join(format!("{}.json", fixture.name));
        std::fs::write(path, serde_json::to_vec_pretty(&fixture.value).unwrap()).unwrap();
    }

    let checked_in_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/fixtures/jsonrpc");
    let generated_files = json_files(generated_dir.path());
    let checked_in_files = json_files(&checked_in_dir);
    assert_eq!(
        generated_files, checked_in_files,
        "fixture file set is stale"
    );

    for file_name in generated_files {
        let generated: serde_json::Value =
            serde_json::from_slice(&std::fs::read(generated_dir.path().join(&file_name)).unwrap())
                .unwrap();
        let checked_in: serde_json::Value =
            serde_json::from_slice(&std::fs::read(checked_in_dir.join(&file_name)).unwrap())
                .unwrap();
        assert_eq!(
            generated,
            checked_in,
            "fixture {} is stale",
            file_name.display()
        );
    }
}

#[test]
fn fixture_messages_include_current_stage_methods() {
    let fixtures = fixture_messages();
    for name in [
        "initialize.request",
        "initialize.response",
        "thread-start.request",
        "fs-read-file.request",
        "command-exec.request",
    ] {
        assert!(
            fixtures.iter().any(|fixture| fixture.name == name),
            "missing fixture {name}"
        );
    }
}

#[test]
fn initialize_fixture_lists_all_server_capabilities() {
    let fixture = fixture_messages()
        .into_iter()
        .find(|fixture| fixture.name == "initialize.response")
        .unwrap();
    assert_eq!(
        fixture.value["result"]["capabilities"],
        serde_json::json!([
            "threads", "turns", "fs", "command", "models", "config", "skills", "hooks", "mcp",
            "plugins"
        ])
    );
}

#[test]
fn protocol_schema_lists_current_stage_methods() {
    let schema = protocol_schema();
    let methods = schema["methods"].as_array().expect("methods array");
    for method in ["initialize", "thread/start", "fs/readFile", "command/exec"] {
        assert!(
            methods.iter().any(|value| value == method),
            "missing method {method}"
        );
    }
}
