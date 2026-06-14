//! Slash command registry. UI-agnostic: the registry maps names to
//! `SlashCommand` descriptors; the REPL/TUI front-ends decide how to
//! present and dispatch the resulting `CommandAction`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAction {
    /// Front-end handles it without touching the engine (help, clear, exit).
    Local,
    /// Needs the engine (async): compact, model switch, cost report.
    Engine,
    /// A UI affordance (theme picker, session picker). Headless prints
    /// "TUI only".
    Ui,
}

#[derive(Debug, Clone, Copy)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    pub action: CommandAction,
}

pub struct CommandRegistry {
    commands: Vec<SlashCommand>,
}

impl CommandRegistry {
    pub fn with_builtins() -> Self {
        Self {
            commands: super::builtin::BUILTINS.to_vec(),
        }
    }

    pub fn all(&self) -> &[SlashCommand] {
        &self.commands
    }

    pub fn get(&self, name: &str) -> Option<&SlashCommand> {
        self.commands.iter().find(|c| c.name == name)
    }

    /// Subsequence-fuzzy prefix match for autocomplete (Phase 05 reuses this).
    pub fn lookup(&self, prefix: &str) -> Vec<&SlashCommand> {
        let p = prefix.trim_start_matches('/');
        self.commands
            .iter()
            .filter(|c| c.name.starts_with(p) || subsequence(p, c.name))
            .collect()
    }
}

/// True if `needle` is a subsequence of `haystack` (case-insensitive).
fn subsequence(needle: &str, haystack: &str) -> bool {
    let mut hi = haystack.chars().map(|c| c.to_ascii_lowercase());
    'outer: for nc in needle.chars().map(|c| c.to_ascii_lowercase()) {
        for hc in hi.by_ref() {
            if hc == nc {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

/// Split "/name rest" -> ("name", "rest"). Returns None if not a slash cmd.
pub fn parse_slash(input: &str) -> Option<(&str, &str)> {
    let s = input.trim();
    let rest = s.strip_prefix('/')?;
    match rest.split_once(char::is_whitespace) {
        Some((name, args)) => Some((name, args.trim())),
        None => Some((rest, "")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_prefix_fuzzy() {
        let reg = CommandRegistry::with_builtins();
        let hits = reg.lookup("co");
        let names: Vec<&str> = hits.iter().map(|c| c.name).collect();
        assert!(names.contains(&"compact"));
        assert!(names.contains(&"config"));
        assert!(names.contains(&"cost"));
        assert!(!names.contains(&"help"));
    }

    #[test]
    fn exact_lookup_finds_help() {
        let reg = CommandRegistry::with_builtins();
        assert!(reg.get("help").is_some());
        assert!(reg.get("nonsense").is_none());
    }

    #[test]
    fn builtins_include_tab_switch_command() {
        let reg = CommandRegistry::with_builtins();
        let tab = reg.get("tab").expect("/tab command should be registered");
        assert_eq!(tab.usage, "/tab [n|next|prev]");
        assert_eq!(tab.description, "Switch session tab");
    }

    #[test]
    fn builtins_include_connect_command() {
        let reg = CommandRegistry::with_builtins();
        let connect = reg
            .get("connect")
            .expect("/connect command should be registered");
        assert_eq!(connect.usage, "/connect");
        assert_eq!(connect.description, "Connect a provider");
    }

    #[test]
    fn builtins_include_sidebar_command() {
        let reg = CommandRegistry::with_builtins();
        let sidebar = reg
            .get("sidebar")
            .expect("/sidebar command should be registered");
        assert_eq!(sidebar.usage, "/sidebar [on|off|toggle|auto]");
        assert_eq!(sidebar.description, "Show or hide the sidebar");
    }

    #[test]
    fn parse_input_splits_name_and_args() {
        assert_eq!(
            parse_slash("/model MiniMax-M1"),
            Some(("model", "MiniMax-M1"))
        );
        assert_eq!(parse_slash("/clear"), Some(("clear", "")));
        assert_eq!(parse_slash("not a command"), None);
    }
}
