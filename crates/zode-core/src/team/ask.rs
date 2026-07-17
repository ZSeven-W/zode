//! `@ask` relay parsing. A teammate asks a colleague by ending a reply line
//! with `@ask <name>: <question>`; the LEADER relays (turn-end, not live —
//! spec ADR-3). Unknown targets and self-asks become warnings, not asks.

#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub to: String,
    pub question: String,
}

/// Parse `@ask` lines out of a teammate reply. Returns `(asks, warnings)`.
pub fn parse_asks(reply: &str, roster: &[String], self_name: &str) -> (Vec<Ask>, Vec<String>) {
    let mut asks = Vec::new();
    let mut warnings = Vec::new();
    for line in reply.lines() {
        let Some(rest) = line.trim_start().strip_prefix("@ask ") else {
            continue;
        };
        let Some((to, question)) = rest.split_once(':') else {
            warnings.push(format!("malformed @ask line: {line}"));
            continue;
        };
        let to = to.trim().to_lowercase();
        let question = question.trim().to_string();
        if to == self_name {
            warnings.push(format!("'{self_name}' asked itself; dropped"));
        } else if !roster.contains(&to) {
            warnings.push(format!("@ask target '{to}' is not on the roster"));
        } else if question.is_empty() {
            warnings.push(format!("@ask to '{to}' has an empty question"));
        } else {
            asks.push(Ask { to, question });
        }
    }
    (asks, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_parsing_rejects_unknown_and_self() {
        let roster = vec!["alice".to_string(), "bob".to_string()];
        let reply = "结论如上。\n@ask bob: 接口签名确认？\n@ask ghost: 在吗\n@ask alice: 自问";
        let (asks, warns) = parse_asks(reply, &roster, "alice");
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].to, "bob");
        assert_eq!(asks[0].question, "接口签名确认？");
        assert_eq!(warns.len(), 2);
    }

    #[test]
    fn malformed_and_empty_asks_warn() {
        let roster = vec!["bob".to_string()];
        let (asks, warns) = parse_asks("@ask bob no colon\n@ask bob:   ", &roster, "alice");
        assert!(asks.is_empty());
        assert_eq!(warns.len(), 2);
    }
}
