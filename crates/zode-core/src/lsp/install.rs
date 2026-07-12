//! On-demand language-server provisioning. A server that isn't already on
//! `PATH` is installed into `~/.zode/lsp` using the ecosystem's own installer
//! (npm for the JS-based servers, rustup/go for the toolchain ones), so the
//! user gets a built-in experience without Zode shipping gigabytes of
//! per-platform binaries. Servers with no automated recipe are offered only
//! when already present.
//!
//! `ensure()` is synchronous (it shells out and waits); callers on an async
//! path run it via `spawn_blocking`.

use std::path::PathBuf;
use std::process::Command;

use crate::config::ConfigManager;

/// How to obtain a server that isn't on `PATH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// `npm install --prefix ~/.zode/lsp <package…>`; the runnable lands at
    /// `~/.zode/lsp/node_modules/.bin/<bin>`. `package` may be space-separated.
    Npm {
        package: &'static str,
        bin: &'static str,
    },
    /// `rustup component add <component>` (then the rustup proxy runs it).
    Rustup { component: &'static str },
    /// `go install <module>@latest` (binary lands in the Go bin dir on PATH).
    Go { module: &'static str },
    /// No automated recipe — system package / SDK. Offered only if on PATH.
    Manual,
}

/// One known language server.
pub struct ServerSpec {
    /// LSP languageId / plugin key (`lsp:<lang>`).
    pub lang: &'static str,
    /// Command to run when resolved from PATH.
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub install: Install,
}

/// Mainstream language servers. Order is the picker's display order.
pub const SERVERS: &[ServerSpec] = &[
    // ---- auto-installable: rustup / go --------------------------------------
    ServerSpec {
        lang: "rust",
        command: "rust-analyzer",
        args: &[],
        extensions: &["rs"],
        install: Install::Rustup {
            component: "rust-analyzer",
        },
    },
    ServerSpec {
        lang: "go",
        command: "gopls",
        args: &[],
        extensions: &["go"],
        install: Install::Go {
            module: "golang.org/x/tools/gopls",
        },
    },
    // ---- auto-installable: npm ----------------------------------------------
    ServerSpec {
        lang: "typescript",
        command: "typescript-language-server",
        args: &["--stdio"],
        extensions: &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
        install: Install::Npm {
            package: "typescript-language-server typescript",
            bin: "typescript-language-server",
        },
    },
    ServerSpec {
        lang: "python",
        command: "pyright-langserver",
        args: &["--stdio"],
        extensions: &["py", "pyi"],
        install: Install::Npm {
            package: "pyright",
            bin: "pyright-langserver",
        },
    },
    ServerSpec {
        lang: "bash",
        command: "bash-language-server",
        args: &["start"],
        extensions: &["sh", "bash"],
        install: Install::Npm {
            package: "bash-language-server",
            bin: "bash-language-server",
        },
    },
    ServerSpec {
        lang: "php",
        command: "intelephense",
        args: &["--stdio"],
        extensions: &["php"],
        install: Install::Npm {
            package: "intelephense",
            bin: "intelephense",
        },
    },
    ServerSpec {
        lang: "json",
        command: "vscode-json-language-server",
        args: &["--stdio"],
        extensions: &["json", "jsonc"],
        install: Install::Npm {
            package: "vscode-langservers-extracted",
            bin: "vscode-json-language-server",
        },
    },
    ServerSpec {
        lang: "html",
        command: "vscode-html-language-server",
        args: &["--stdio"],
        extensions: &["html", "htm"],
        install: Install::Npm {
            package: "vscode-langservers-extracted",
            bin: "vscode-html-language-server",
        },
    },
    ServerSpec {
        lang: "css",
        command: "vscode-css-language-server",
        args: &["--stdio"],
        extensions: &["css", "scss", "less"],
        install: Install::Npm {
            package: "vscode-langservers-extracted",
            bin: "vscode-css-language-server",
        },
    },
    ServerSpec {
        lang: "yaml",
        command: "yaml-language-server",
        args: &["--stdio"],
        extensions: &["yaml", "yml"],
        install: Install::Npm {
            package: "yaml-language-server",
            bin: "yaml-language-server",
        },
    },
    // ---- manual: offered only when already on PATH --------------------------
    ServerSpec {
        lang: "cpp",
        command: "clangd",
        args: &[],
        extensions: &["c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx", "m", "mm"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "zig",
        command: "zls",
        args: &[],
        extensions: &["zig"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "swift",
        command: "sourcekit-lsp",
        args: &[],
        extensions: &["swift"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "java",
        command: "jdtls",
        args: &[],
        extensions: &["java"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "kotlin",
        command: "kotlin-language-server",
        args: &[],
        extensions: &["kt", "kts"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "scala",
        command: "metals",
        args: &[],
        extensions: &["scala", "sbt", "sc"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "clojure",
        command: "clojure-lsp",
        args: &[],
        extensions: &["clj", "cljs", "cljc", "edn"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "csharp",
        command: "csharp-ls",
        args: &[],
        extensions: &["cs"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "ruby",
        command: "solargraph",
        args: &["stdio"],
        extensions: &["rb", "rake", "gemspec"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "lua",
        command: "lua-language-server",
        args: &[],
        extensions: &["lua"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "haskell",
        command: "haskell-language-server-wrapper",
        args: &["--lsp"],
        extensions: &["hs", "lhs"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "ocaml",
        command: "ocamllsp",
        args: &[],
        extensions: &["ml", "mli"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "elixir",
        command: "elixir-ls",
        args: &[],
        extensions: &["ex", "exs"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "dart",
        command: "dart",
        args: &["language-server"],
        extensions: &["dart"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "toml",
        command: "taplo",
        args: &["lsp", "stdio"],
        extensions: &["toml"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "terraform",
        command: "terraform-ls",
        args: &["serve"],
        extensions: &["tf", "tfvars"],
        install: Install::Manual,
    },
    ServerSpec {
        lang: "markdown",
        command: "marksman",
        args: &[],
        extensions: &["md", "markdown"],
        install: Install::Manual,
    },
];

pub fn spec_for_lang(lang: &str) -> Option<&'static ServerSpec> {
    SERVERS.iter().find(|s| s.lang == lang)
}

/// The managed install root: `<config-dir>/lsp` (honors `ZODE_CONFIG_DIR`).
pub fn install_root() -> Option<PathBuf> {
    ConfigManager::config_dir().ok().map(|d| d.join("lsp"))
}

/// Candidate file names for a command: as-is on Unix; on Windows the
/// launcher extensions come first (npm shims are `.cmd`, compiled tools
/// `.exe` — a bare `foo` almost never exists there, so without these the
/// auto-detection finds nothing on Windows).
fn candidate_names(cmd: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{cmd}.exe"),
            format!("{cmd}.cmd"),
            format!("{cmd}.bat"),
            cmd.to_string(),
        ]
    } else {
        vec![cmd.to_string()]
    }
}

fn first_file_in(dir: &std::path::Path, cmd: &str) -> Option<PathBuf> {
    candidate_names(cmd)
        .into_iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

/// Full path of `cmd` on `PATH`, if any.
pub fn find_on_path(cmd: &str) -> Option<PathBuf> {
    find_on_path_excluding(cmd, None)
}

/// Full path of `cmd` on `PATH`, ignoring one known-bad hit (a dead rustup
/// proxy shim — see [`rustup_resolved`]). `~/.cargo/bin` is normally on `PATH`,
/// so without this exclusion the shim we just rejected would come straight back.
fn find_on_path_excluding(cmd: &str, skip: Option<&std::path::Path>) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter_map(|dir| first_file_in(&dir, cmd))
        .find(|p| Some(p.as_path()) != skip)
}

/// Whether `cmd` resolves to a file on `PATH`.
pub fn on_path(cmd: &str) -> bool {
    find_on_path(cmd).is_some()
}

/// Directories `go install` drops binaries into, cheapest first: `$GOBIN`,
/// `$GOPATH/bin`, `~/go/bin` (the default when neither is set). Version
/// managers (gvm, asdf) often leave these off `PATH`, so a SUCCESSFUL
/// install would otherwise read as "not found" — and get re-installed on
/// every first use.
fn go_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(gobin) = std::env::var_os("GOBIN") {
        dirs.push(PathBuf::from(gobin));
    }
    if let Some(gopath) = std::env::var_os("GOPATH") {
        dirs.extend(std::env::split_paths(&gopath).map(|p| p.join("bin")));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("go").join("bin"));
    }
    dirs
}

/// Same, but asking `go env` (subprocess — only used on the blocking
/// install path, where the `GOPATH` env var may be unset while the go
/// toolchain still has one configured).
fn go_env_bin_dirs() -> Vec<PathBuf> {
    let Ok(out) = Command::new("go").args(["env", "GOBIN", "GOPATH"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let gobin = lines.next().unwrap_or("").trim();
    let gopath = lines.next().unwrap_or("").trim();
    let mut dirs = Vec::new();
    if !gobin.is_empty() {
        dirs.push(PathBuf::from(gobin));
    }
    if !gopath.is_empty() {
        dirs.extend(std::env::split_paths(std::ffi::OsStr::new(gopath)).map(|p| p.join("bin")));
    }
    dirs
}

/// `$CARGO_HOME/bin` / `~/.cargo/bin` — where the rustup proxies live even
/// when the user's `PATH` doesn't include them.
fn cargo_bin_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        return Some(PathBuf::from(home).join("bin"));
    }
    dirs::home_dir().map(|h| h.join(".cargo").join("bin"))
}

/// Whether `component` appears in `rustup component list --installed` output.
/// Lines are target-suffixed (`rust-analyzer-aarch64-apple-darwin`), so match
/// the exact name or the name followed by the `-<target>` separator — never a
/// bare prefix, which would let `rustc` satisfy a query for `rust`.
fn component_listed(stdout: &str, component: &str) -> bool {
    stdout.lines().map(str::trim).any(|line| {
        line == component
            || line
                .strip_prefix(component)
                .is_some_and(|rest| rest.starts_with('-'))
    })
}

/// Whether rustup reports `component` as installed. Not cached: `ensure()` asks
/// again right after `rustup component add`, and a stale "no" there would make
/// a successful install read as a failure.
fn rustup_component_installed(component: &str) -> bool {
    Command::new("rustup")
        .args(["component", "list", "--installed"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| component_listed(&String::from_utf8_lossy(&o.stdout), component))
}

/// Resolve a rustup-provided command, given its `~/.cargo/bin` proxy shim (if
/// present) and whether the component is actually installed.
///
/// rustup pre-creates a proxy shim for every component it knows about, so the
/// shim's existence says nothing about the component: running the one for a
/// missing component just prints `error: Unknown binary 'rust-analyzer' in
/// official toolchain` and exits. Only trust it once rustup confirms the
/// component; otherwise ignore it (including the `PATH` hit that resolves back
/// to it) so a real binary installed by other means still wins, and `ensure()`
/// can run `rustup component add` when there is none.
fn rustup_resolved(command: &str, shim: Option<PathBuf>, installed: bool) -> Option<PathBuf> {
    if installed {
        if let Some(p) = shim {
            return Some(p);
        }
    }
    find_on_path_excluding(command, shim.as_deref())
}

/// The runnable path for a spec if it's already available: the npm-managed
/// bin under `~/.zode/lsp`, the recipe's install destination (go bin dirs /
/// cargo bin), or the command on `PATH`.
pub fn resolve(spec: &ServerSpec) -> Option<PathBuf> {
    match spec.install {
        Install::Npm { bin, .. } => {
            if let Some(root) = install_root() {
                if let Some(managed) = first_file_in(&root.join("node_modules").join(".bin"), bin) {
                    return Some(managed);
                }
            }
        }
        Install::Go { .. } => {
            for dir in go_bin_dirs() {
                if let Some(p) = first_file_in(&dir, spec.command) {
                    return Some(p);
                }
            }
        }
        Install::Rustup { component } => {
            let shim = cargo_bin_dir().and_then(|d| first_file_in(&d, spec.command));
            let installed = shim.is_some() && rustup_component_installed(component);
            return rustup_resolved(spec.command, shim, installed);
        }
        Install::Manual => {}
    }
    find_on_path(spec.command)
}

/// Whether the installer this spec needs (npm / rustup / go) is on `PATH`.
pub fn installable(spec: &ServerSpec) -> bool {
    match spec.install {
        Install::Npm { .. } => on_path("npm"),
        Install::Rustup { .. } => on_path("rustup"),
        Install::Go { .. } => on_path("go"),
        Install::Manual => false,
    }
}

/// A spec is offered to the user (picker + routing) when it's already runnable
/// or we have a usable installer for it.
pub fn offered(spec: &ServerSpec) -> bool {
    resolve(spec).is_some() || installable(spec)
}

/// Ensure the server is runnable, installing it on demand. Returns the command
/// path to spawn. Blocks while installing — run via `spawn_blocking`.
pub fn ensure(spec: &ServerSpec) -> Result<PathBuf, String> {
    if let Some(path) = resolve(spec) {
        return Ok(path);
    }
    if !installable(spec) {
        return Err(format!(
            "{} is not installed and has no automated installer; install it manually",
            spec.command
        ));
    }
    run_install(spec)?;
    if let Some(path) = resolve(spec) {
        return Ok(path);
    }
    // A Go install may have landed in a bin dir only the toolchain knows
    // about (`go env GOPATH` set via config, not the environment).
    if matches!(spec.install, Install::Go { .. }) {
        for dir in go_env_bin_dirs() {
            if let Some(p) = first_file_in(&dir, spec.command) {
                return Ok(p);
            }
        }
    }
    Err(format!(
        "installed {} but could not locate the binary",
        spec.command
    ))
}

fn run_install(spec: &ServerSpec) -> Result<(), String> {
    let output = match spec.install {
        Install::Npm { package, .. } => {
            let root = install_root().ok_or("no config dir for ~/.zode/lsp")?;
            std::fs::create_dir_all(&root).map_err(|e| format!("mkdir {}: {e}", root.display()))?;
            tracing::info!("installing language server: npm install {package}");
            // Spawn npm by its full path: on Windows it's `npm.cmd`, which
            // `Command::new("npm")` would not find.
            let npm = find_on_path("npm").unwrap_or_else(|| PathBuf::from("npm"));
            let mut cmd = Command::new(npm);
            cmd.arg("install").arg("--prefix").arg(&root);
            for pkg in package.split_whitespace() {
                cmd.arg(pkg);
            }
            cmd.output()
        }
        Install::Rustup { component } => {
            tracing::info!("installing language server: rustup component add {component}");
            Command::new("rustup")
                .args(["component", "add", component])
                .output()
        }
        Install::Go { module } => {
            tracing::info!("installing language server: go install {module}@latest");
            Command::new("go")
                .arg("install")
                .arg(format!("{module}@latest"))
                .output()
        }
        Install::Manual => return Err("no automated installer".into()),
    };
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join("; ");
            Err(format!("install of {} failed: {tail}", spec.command))
        }
        Err(e) => Err(format!("could not run installer for {}: {e}", spec.command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_unique_and_have_extensions() {
        let mut seen = std::collections::HashSet::new();
        for s in SERVERS {
            assert!(seen.insert(s.lang), "duplicate lang {}", s.lang);
            assert!(!s.extensions.is_empty(), "{} has no extensions", s.lang);
        }
        assert!(spec_for_lang("typescript").is_some());
        assert!(spec_for_lang("nope").is_none());
    }

    #[test]
    fn installable_matches_recipe_kind() {
        let manual = spec_for_lang("cpp").unwrap();
        assert_eq!(manual.install, Install::Manual);
        assert!(!installable(manual)); // Manual is never auto-installable
    }

    #[test]
    #[serial_test::serial]
    fn resolve_finds_go_binaries_outside_path() {
        // Regression: `go install` drops gopls into $GOBIN / $GOPATH/bin,
        // which version managers often leave off PATH. resolve() must look
        // there — otherwise a successful install reads as "not found" and
        // ensure() re-installs on every first use.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("gopls");
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        let spec = spec_for_lang("go").unwrap();

        let old_gobin = std::env::var_os("GOBIN");
        std::env::set_var("GOBIN", dir.path());
        let found = resolve(spec);
        match old_gobin {
            Some(v) => std::env::set_var("GOBIN", v),
            None => std::env::remove_var("GOBIN"),
        }
        assert_eq!(found, Some(fake));
    }

    #[test]
    #[serial_test::serial]
    fn rustup_proxy_shim_is_not_mistaken_for_an_installed_component() {
        // Regression: rustup pre-creates a proxy shim in ~/.cargo/bin for EVERY
        // component it knows about, installed or not — running the one for a
        // missing component just errors ("Unknown binary 'rust-analyzer' in
        // official toolchain"). Treating the shim's mere existence as "already
        // installed" short-circuits `ensure()`, so `rustup component add` never
        // runs and the spawn dies instantly.
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("rust-analyzer");
        std::fs::write(&shim, b"#!/bin/sh\n").unwrap();
        // An empty PATH, so the developer's own rust-analyzer can't answer for
        // the fixture.
        let empty = tempfile::tempdir().unwrap();
        let old_path = std::env::var_os("PATH");
        std::env::set_var("PATH", empty.path());

        // Component NOT installed → the shim must not resolve.
        let absent = rustup_resolved("rust-analyzer", Some(shim.clone()), false);
        // Component installed → the shim is the real proxy, use it.
        let present = rustup_resolved("rust-analyzer", Some(shim.clone()), true);

        match old_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        assert_eq!(absent, None);
        assert_eq!(present, Some(shim));
    }

    #[test]
    #[serial_test::serial]
    fn a_real_binary_on_path_wins_over_a_dead_rustup_shim() {
        // rust-analyzer installed by other means (brew, manual) while the rustup
        // component is absent: the dead shim must be skipped, not preferred.
        let cargo_bin = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let shim = cargo_bin.path().join("rust-analyzer");
        let real = other.path().join("rust-analyzer");
        std::fs::write(&shim, b"#!/bin/sh\n").unwrap();
        std::fs::write(&real, b"#!/bin/sh\n").unwrap();

        let old_path = std::env::var_os("PATH");
        let joined =
            std::env::join_paths([cargo_bin.path().to_path_buf(), other.path().to_path_buf()])
                .unwrap();
        std::env::set_var("PATH", &joined);
        let found = rustup_resolved("rust-analyzer", Some(shim), false);
        match old_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        assert_eq!(found, Some(real));
    }

    #[test]
    fn parses_installed_components_with_target_suffixes() {
        let out = "cargo-aarch64-apple-darwin\n\
                   clippy-aarch64-apple-darwin\n\
                   rust-analyzer-aarch64-apple-darwin\n\
                   rustc-aarch64-apple-darwin\n";
        assert!(component_listed(out, "rust-analyzer"));
        assert!(component_listed(out, "clippy"));
        assert!(!component_listed(out, "rustfmt"));
        // A partial name must not match: only the exact component, or the
        // component followed by its `-<target>` suffix, counts.
        assert!(!component_listed(out, "rust-anal"));
        assert!(!component_listed(out, "car"));
    }

    #[test]
    fn candidate_names_are_platform_shaped() {
        let names = candidate_names("tool");
        if cfg!(windows) {
            assert!(names.contains(&"tool.exe".to_string()));
            assert!(names.contains(&"tool.cmd".to_string()));
        } else {
            assert_eq!(names, vec!["tool".to_string()]);
        }
    }

    #[test]
    fn ensure_errors_clearly_when_manual_and_absent() {
        // A Manual server whose command is surely absent yields a clear error,
        // not a panic. (Uses a spec we know is Manual.)
        let spec = spec_for_lang("markdown").unwrap();
        if resolve(spec).is_none() {
            let err = ensure(spec).unwrap_err();
            assert!(err.contains("manually"), "{err}");
        }
    }
}
