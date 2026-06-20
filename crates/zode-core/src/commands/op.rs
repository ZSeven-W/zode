//! Parse `/op <subcommand> …`. `status` is a zode-side connection report; the
//! rest map to an MCP (tool, arguments) pair.

use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq)]
pub enum OpCommand {
    Status,
    Call {
        tool: String,
        args: Value,
    },
    /// Run the design-pipeline orchestrator from a natural-language prompt.
    Generate {
        prompt: String,
    },
}

pub fn map_subcommand(args: &str) -> Result<OpCommand, String> {
    let args = args.trim();
    let (head, rest) = match args.split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim()),
        None => (args, ""),
    };
    match head {
        "" => Err("usage: /op <status|design '<dsl>'|<tool> <json>>".into()),
        "status" => Ok(OpCommand::Status),
        "design" => {
            if rest.is_empty() {
                return Err("usage: /op design '<dsl>'".into());
            }
            Ok(OpCommand::Call {
                tool: "batch_design".into(),
                args: json!({ "dsl": rest }),
            })
        }
        "call" => {
            let (tool, payload) = rest.split_once(char::is_whitespace).unwrap_or((rest, "{}"));
            if tool.is_empty() {
                return Err("usage: /op call <tool> <json>".into());
            }
            let v: Value = serde_json::from_str(payload.trim()).map_err(|e| e.to_string())?;
            Ok(OpCommand::Call {
                tool: tool.into(),
                args: v,
            })
        }
        "generate" => {
            if rest.is_empty() {
                return Err("usage: /op generate <prompt>".into());
            }
            Ok(OpCommand::Generate {
                prompt: rest.to_string(),
            })
        }
        tool => {
            let v: Value = if rest.is_empty() {
                json!({})
            } else {
                serde_json::from_str(rest).map_err(|e| format!("bad JSON args: {e}"))?
            };
            Ok(OpCommand::Call {
                tool: tool.into(),
                args: v,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_design_call_passthrough() {
        assert!(matches!(
            map_subcommand("status").unwrap(),
            OpCommand::Status
        ));
        match map_subcommand("design F1=I(\"p\",{})").unwrap() {
            OpCommand::Call { tool, args } => {
                assert_eq!(tool, "batch_design");
                assert_eq!(args["dsl"], "F1=I(\"p\",{})");
            }
            _ => panic!(),
        }
        match map_subcommand("call insert_node {\"x\":1}").unwrap() {
            OpCommand::Call { tool, args } => {
                assert_eq!(tool, "insert_node");
                assert_eq!(args["x"], 1);
            }
            _ => panic!(),
        }
        match map_subcommand("get_document_info").unwrap() {
            OpCommand::Call { tool, .. } => assert_eq!(tool, "get_document_info"),
            _ => panic!(),
        }
    }

    #[test]
    fn maps_generate() {
        match map_subcommand("generate a pricing page").unwrap() {
            OpCommand::Generate { prompt } => assert_eq!(prompt, "a pricing page"),
            _ => panic!(),
        }
        assert!(map_subcommand("generate").is_err()); // needs a prompt
    }

    #[test]
    fn empty_errs() {
        assert!(map_subcommand("").is_err());
    }
}
