use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX_POLICY_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsTier {
    Auto,
    Basic,
    Elevated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWindowsTier {
    pub tier: WindowsTier,
    pub notice: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsPolicy {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub writable_roots: Vec<PathBuf>,
    pub read_only: bool,
    #[serde(default)]
    pub network_enforced: bool,
}

pub fn parse_windows_tier(value: Option<&str>) -> ResolvedWindowsTier {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("basic") => ResolvedWindowsTier {
            tier: WindowsTier::Basic,
            notice: None,
        },
        Some("elevated" | "appcontainer" | "strict") => ResolvedWindowsTier {
            tier: WindowsTier::Elevated,
            notice: None,
        },
        _ => ResolvedWindowsTier {
            tier: WindowsTier::Auto,
            notice: None,
        },
    }
}

pub fn resolve_network_enforcement(requested: WindowsTier) -> bool {
    match requested {
        WindowsTier::Basic | WindowsTier::Auto => false,
        WindowsTier::Elevated => true,
    }
}

pub fn encode_policy(policy: &WindowsPolicy) -> Result<Vec<u8>, String> {
    let encoded = serde_json::to_vec(policy).map_err(|e| format!("invalid sandbox policy: {e}"))?;
    if encoded.len() > MAX_POLICY_BYTES {
        return Err(format!(
            "sandbox policy is too large ({} bytes; maximum {MAX_POLICY_BYTES})",
            encoded.len()
        ));
    }
    Ok(encoded)
}

pub fn decode_policy(bytes: &[u8]) -> Result<WindowsPolicy, String> {
    if bytes.len() > MAX_POLICY_BYTES {
        return Err(format!(
            "sandbox policy is too large ({} bytes; maximum {MAX_POLICY_BYTES})",
            bytes.len()
        ));
    }
    serde_json::from_slice(bytes).map_err(|e| format!("invalid sandbox policy: {e}"))
}

pub fn normalize_windows_path(path: &Path) -> Result<String, String> {
    let raw = path.to_string_lossy().replace('/', "\\");
    // `std::fs::canonicalize` yields extended-length ("verbatim") paths on
    // Windows, e.g. `\\?\C:\Users\...`. Strip that prefix so the drive-letter
    // validation below sees a plain `C:\...`. A `\\?\UNC\...` remainder keeps
    // its `UNC\` and is rejected as a non-drive path (remote paths unsupported).
    let raw = raw.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(raw);
    let bytes = raw.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return Err(format!(
            "Windows sandbox root must be an absolute drive path: {raw}"
        ));
    }
    let drive = (bytes[0] as char).to_ascii_uppercase();
    let mut parts: Vec<&str> = Vec::new();
    for part in raw[3..].split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("Windows sandbox root escapes its drive: {raw}"));
                }
            }
            value => parts.push(value),
        }
    }
    let suffix = parts.join("\\");
    Ok(if suffix.is_empty() {
        format!("{drive}:\\")
    } else {
        format!("{drive}:\\{suffix}")
    })
}

/// Removes only the Win32 extended-length marker used by canonicalized local
/// paths. Process current directories must name the existing path exactly;
/// unlike policy normalization, launch formatting must not fold components or
/// change the drive spelling.
#[cfg(windows)]
pub(crate) fn strip_verbatim_prefix(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

pub fn capability_sid_components(normalized_root: &str) -> [u32; 4] {
    let mut state = [
        0x811c9dc5_u32,
        0x9e3779b9_u32,
        0x85ebca6b_u32,
        0xc2b2ae35_u32,
    ];
    for (index, byte) in normalized_root
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
        .bytes()
        .enumerate()
    {
        for (lane, value) in state.iter_mut().enumerate() {
            *value ^=
                u32::from(byte).wrapping_add(((index + lane) as u32).rotate_left(lane as u32));
            *value = value
                .wrapping_mul(0x0100_0193)
                .rotate_left((lane + 3) as u32);
        }
    }
    state
}

pub fn tier_one_summary(read_only: bool) -> String {
    let writes = if read_only {
        "best-effort read-only write confinement"
    } else {
        "best-effort write confinement to configured roots"
    };
    format!(
        "Windows Tier 1 sandbox (experimental): {writes}; network unenforced. \
         File tools reject .git and .zode paths, but the kernel policy cannot protect them \
         from rename/delete through their parent because parent delete-child rights are retained \
         for build-tool and atomic-rename compatibility."
    )
}

pub fn tier_two_summary(read_only: bool) -> String {
    let writes = if read_only {
        "best-effort read-only write confinement"
    } else {
        "best-effort write confinement to configured roots"
    };
    format!(
        "Windows Tier 2 sandbox (experimental): {writes}; network denied (AppContainer — no \
         network capability, loopback included). \
         File tools reject .git and .zode paths, but the kernel write policy cannot protect them \
         from rename/delete through their parent because parent delete-child rights are retained \
         for build-tool and atomic-rename compatibility."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> WindowsPolicy {
        WindowsPolicy {
            argv: vec!["tool.exe".into(), "two words".into()],
            cwd: PathBuf::from(r"C:\work"),
            writable_roots: vec![PathBuf::from(r"C:\work")],
            read_only: false,
            network_enforced: false,
        }
    }

    #[test]
    fn parses_all_windows_tier_requests() {
        let resolved = parse_windows_tier(Some("elevated"));
        assert_eq!(resolved.tier, WindowsTier::Elevated);
        assert_eq!(resolved.notice, None);
        assert_eq!(parse_windows_tier(None).tier, WindowsTier::Auto);
        assert_eq!(parse_windows_tier(Some("basic")).tier, WindowsTier::Basic);
        assert_eq!(
            parse_windows_tier(Some("appcontainer")).tier,
            WindowsTier::Elevated
        );
        assert_eq!(
            parse_windows_tier(Some("strict")).tier,
            WindowsTier::Elevated
        );
    }

    #[test]
    fn tier_resolution_is_conservative_unless_explicit() {
        assert!(!resolve_network_enforcement(WindowsTier::Basic));
        assert!(!resolve_network_enforcement(WindowsTier::Auto));
        assert!(resolve_network_enforcement(WindowsTier::Elevated));
    }

    #[test]
    fn policy_round_trips_and_rejects_oversize_input() {
        let policy = policy();
        let encoded = encode_policy(&policy).unwrap();
        assert_eq!(decode_policy(&encoded).unwrap(), policy);
        assert!(decode_policy(&vec![b'x'; MAX_POLICY_BYTES + 1]).is_err());
    }

    #[test]
    fn normalizes_windows_paths_lexically() {
        assert_eq!(
            normalize_windows_path(Path::new(r"c:/work/./src/../out\")).unwrap(),
            r"C:\work\out"
        );
        // canonicalize() extended-length prefix is stripped before validation.
        assert_eq!(
            normalize_windows_path(Path::new(r"\\?\C:\work\out")).unwrap(),
            r"C:\work\out"
        );
        assert!(normalize_windows_path(Path::new(r"work\relative")).is_err());
        assert!(normalize_windows_path(Path::new(r"C:\work\..\..")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn launch_path_only_strips_the_verbatim_prefix() {
        let path = r"\\?\c:\Users\RUNNER~1\Temp\.tmp5CO4yQ\a\..\workspace";
        assert_eq!(
            strip_verbatim_prefix(path),
            r"c:\Users\RUNNER~1\Temp\.tmp5CO4yQ\a\..\workspace"
        );
        assert_eq!(
            strip_verbatim_prefix(r"C:\Users\runneradmin\.tmp"),
            r"C:\Users\runneradmin\.tmp"
        );
    }

    #[test]
    fn capability_sid_is_stable_and_root_specific() {
        let a = capability_sid_components(r"C:\work");
        assert_eq!(a, capability_sid_components(r"c:\WORK\"));
        assert_ne!(a, capability_sid_components(r"C:\other"));
        assert_ne!(a, [0; 4]);
    }

    #[test]
    fn tier_one_summary_states_all_experimental_limitations() {
        let summary = tier_one_summary(false);
        for required in [
            "experimental",
            "best-effort",
            "network unenforced",
            ".git",
            ".zode",
            "rename/delete through their parent",
        ] {
            assert!(
                summary.contains(required),
                "missing {required:?}: {summary}"
            );
        }
        assert!(!summary.contains("all writes outside"), "{summary}");
    }

    #[test]
    fn tier_two_summary_states_appcontainer_network_denial() {
        let summary = tier_two_summary(false);
        assert!(
            summary.contains("network denied (AppContainer"),
            "{summary}"
        );
        assert!(summary.contains("loopback included"), "{summary}");
        assert!(!summary.contains("network unenforced"), "{summary}");
    }
}
