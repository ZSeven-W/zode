//! Parse `/op <design request>`. `status` is a zode-side connection report;
//! `generate`, `design`, and `call` remain compatibility paths.

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
        "" => Err("usage: /op <design request>".into()),
        "status" => Ok(OpCommand::Status),
        "design" => {
            if rest.is_empty() {
                return Err("usage: /op design '<operations>'".into());
            }
            Ok(OpCommand::Call {
                tool: "batch_design".into(),
                args: json!({ "operations": rest }),
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
        _ => Ok(OpCommand::Generate {
            prompt: args.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_design_call_compatibility_paths() {
        assert!(matches!(
            map_subcommand("status").unwrap(),
            OpCommand::Status
        ));
        match map_subcommand("design F1=I(\"p\",{})").unwrap() {
            OpCommand::Call { tool, args } => {
                assert_eq!(tool, "batch_design");
                assert_eq!(args["operations"], "F1=I(\"p\",{})");
                assert!(args.get("dsl").is_none());
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
    }

    #[test]
    fn natural_language_maps_to_generate() {
        for input in [
            "a pricing dashboard",
            "get_document_info",
            "做一个移动端首页",
        ] {
            match map_subcommand(input).unwrap() {
                OpCommand::Generate { prompt } => assert_eq!(prompt, input),
                other => panic!("expected Generate for {input:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn maps_generate_alias() {
        match map_subcommand("generate a pricing page").unwrap() {
            OpCommand::Generate { prompt } => assert_eq!(prompt, "a pricing page"),
            _ => panic!(),
        }
        assert!(map_subcommand("generate").is_err());
    }

    #[test]
    fn empty_errs() {
        assert!(map_subcommand("").is_err());
    }
}
