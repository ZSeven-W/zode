use crate::schema::{fixture_messages, protocol_schema};

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
