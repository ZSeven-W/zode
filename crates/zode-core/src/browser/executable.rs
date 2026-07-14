use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::config::BrowserConfig;

use super::backend::BrowserError;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchSpec {
    program: PathBuf,
    args: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserPlatform {
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "constructed by injected cross-platform tests")
    )]
    Macos,
    #[cfg_attr(
        not(target_os = "linux"),
        allow(dead_code, reason = "constructed by injected cross-platform tests")
    )]
    Linux,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "constructed by injected cross-platform tests")
    )]
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserPurpose {
    Managed,
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "reserved for the shared Chrome-only caller")
    )]
    ChromeOnly,
}

#[derive(Debug, Clone, Default)]
struct BrowserEnvironment {
    local_app_data: Option<OsString>,
    program_files: Option<OsString>,
    program_files_x86: Option<OsString>,
    path: Option<OsString>,
}

impl BrowserEnvironment {
    fn current() -> Self {
        Self {
            local_app_data: std::env::var_os("LOCALAPPDATA"),
            program_files: std::env::var_os("ProgramFiles"),
            program_files_x86: std::env::var_os("ProgramFiles(x86)"),
            path: std::env::var_os("PATH"),
        }
    }
}

fn current_platform() -> BrowserPlatform {
    #[cfg(target_os = "macos")]
    return BrowserPlatform::Macos;
    #[cfg(target_os = "linux")]
    return BrowserPlatform::Linux;
    #[cfg(target_os = "windows")]
    return BrowserPlatform::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    compile_error!("managed browser executable lookup is unsupported on this platform");
}

fn push_from_environment(candidates: &mut Vec<PathBuf>, root: &Option<OsString>, relative: &Path) {
    if let Some(root) = root {
        candidates.push(PathBuf::from(root).join(relative));
    }
}

fn push_from_path(candidates: &mut Vec<PathBuf>, env: &BrowserEnvironment, names: &[&str]) {
    let Some(path) = &env.path else {
        return;
    };
    let directories: Vec<_> = std::env::split_paths(path).collect();
    for name in names {
        for directory in &directories {
            candidates.push(directory.join(name));
        }
    }
}

fn deduplicate(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

fn platform_candidates(
    platform: BrowserPlatform,
    purpose: BrowserPurpose,
    env: &BrowserEnvironment,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    match platform {
        BrowserPlatform::Macos => {
            candidates.push(PathBuf::from(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ));
            if purpose == BrowserPurpose::Managed {
                candidates.push(PathBuf::from(
                    "/Applications/Chromium.app/Contents/MacOS/Chromium",
                ));
                candidates.push(PathBuf::from(
                    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                ));
            }
        }
        BrowserPlatform::Linux => {
            let names = if purpose == BrowserPurpose::Managed {
                &[
                    "google-chrome",
                    "google-chrome-stable",
                    "chromium",
                    "chromium-browser",
                    "microsoft-edge",
                ][..]
            } else {
                &["google-chrome", "google-chrome-stable"][..]
            };
            push_from_path(&mut candidates, env, names);
        }
        BrowserPlatform::Windows => {
            let chrome_install = Path::new("Google")
                .join("Chrome")
                .join("Application")
                .join("chrome.exe");
            let edge_install = Path::new("Microsoft")
                .join("Edge")
                .join("Application")
                .join("msedge.exe");

            push_from_environment(&mut candidates, &env.local_app_data, &chrome_install);
            push_from_environment(&mut candidates, &env.program_files, &chrome_install);
            push_from_environment(&mut candidates, &env.program_files_x86, &chrome_install);
            push_from_path(&mut candidates, env, &["chrome.exe", "google-chrome.exe"]);

            if purpose == BrowserPurpose::Managed {
                push_from_environment(&mut candidates, &env.program_files, &edge_install);
                push_from_environment(&mut candidates, &env.program_files_x86, &edge_install);
                push_from_path(&mut candidates, env, &["msedge.exe"]);
            }
        }
    }
    deduplicate(candidates)
}

fn locate_executable_with(
    cfg: &BrowserConfig,
    platform: BrowserPlatform,
    purpose: BrowserPurpose,
    env: &BrowserEnvironment,
    is_file: impl Fn(&Path) -> bool,
) -> Result<PathBuf, BrowserError> {
    let mut candidates = cfg
        .executable
        .as_ref()
        .map(PathBuf::from)
        .into_iter()
        .collect::<Vec<_>>();
    candidates.extend(platform_candidates(platform, purpose, env));
    let candidates = deduplicate(candidates);

    if let Some(executable) = candidates.iter().find(|path| is_file(path)) {
        return Ok(executable.clone());
    }

    let label = match purpose {
        BrowserPurpose::Managed => "Chrome/Chromium/Edge",
        BrowserPurpose::ChromeOnly => "Google Chrome",
    };
    let tried = candidates
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(", ");
    Err(BrowserError::NotFound(format!(
        "no {label} executable found; tried: [{tried}]; set browser.executable in config"
    )))
}

fn extension_launch_specs_with(
    cfg: &BrowserConfig,
    platform: BrowserPlatform,
    env: &BrowserEnvironment,
    url: &str,
    is_file: impl Fn(&Path) -> bool,
) -> Result<Vec<LaunchSpec>, BrowserError> {
    let url = OsString::from(url);
    match platform {
        BrowserPlatform::Windows => {
            let program =
                locate_executable_with(cfg, platform, BrowserPurpose::ChromeOnly, env, is_file)?;
            Ok(vec![LaunchSpec {
                program,
                args: vec![url],
            }])
        }
        BrowserPlatform::Macos => Ok(vec![
            LaunchSpec {
                program: PathBuf::from("open"),
                args: vec![
                    OsString::from("-a"),
                    OsString::from("Google Chrome"),
                    url.clone(),
                ],
            },
            LaunchSpec {
                program: PathBuf::from("open"),
                args: vec![url],
            },
        ]),
        BrowserPlatform::Linux => Ok(
            ["google-chrome", "chromium", "chromium-browser", "xdg-open"]
                .into_iter()
                .map(|program| LaunchSpec {
                    program: PathBuf::from(program),
                    args: vec![url.clone()],
                })
                .collect(),
        ),
    }
}

async fn launch_specs_with<F, Fut>(specs: Vec<LaunchSpec>, mut spawn: F) -> Result<(), BrowserError>
where
    F: FnMut(LaunchSpec) -> Fut,
    Fut: std::future::Future<Output = std::io::Result<()>>,
{
    let mut failures = Vec::new();

    for spec in specs {
        let program = spec.program.clone();
        match spawn(spec).await {
            Ok(()) => return Ok(()),
            Err(error) => failures.push(format!("{}: {error}", program.display())),
        }
    }

    if failures.is_empty() {
        Err(BrowserError::Launch(
            "no browser opener configured for this platform".into(),
        ))
    } else {
        Err(BrowserError::Launch(failures.join("; ")))
    }
}

pub(super) fn locate_managed_executable(cfg: &BrowserConfig) -> Result<PathBuf, BrowserError> {
    locate_executable_with(
        cfg,
        current_platform(),
        BrowserPurpose::Managed,
        &BrowserEnvironment::current(),
        |path| path.is_file(),
    )
}

pub(super) async fn open_extension_url(cfg: &BrowserConfig, url: &str) -> Result<(), BrowserError> {
    let specs = extension_launch_specs_with(
        cfg,
        current_platform(),
        &BrowserEnvironment::current(),
        url,
        Path::is_file,
    )?;

    launch_specs_with(specs, |spec| async move {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn()?;
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ffi::OsString;
    use std::io;
    use std::path::{Path, PathBuf};

    use crate::config::BrowserConfig;

    use super::*;

    fn windows_environment() -> BrowserEnvironment {
        BrowserEnvironment {
            local_app_data: Some(OsString::from("/local")),
            program_files: Some(OsString::from("/program-files")),
            program_files_x86: Some(OsString::from("/program-files-x86")),
            path: Some(
                std::env::join_paths([PathBuf::from("/path-one"), PathBuf::from("/path-two")])
                    .unwrap(),
            ),
        }
    }

    fn windows_chrome_candidates() -> Vec<PathBuf> {
        [
            "/local/Google/Chrome/Application/chrome.exe",
            "/program-files/Google/Chrome/Application/chrome.exe",
            "/program-files-x86/Google/Chrome/Application/chrome.exe",
            "/path-one/chrome.exe",
            "/path-two/chrome.exe",
            "/path-one/google-chrome.exe",
            "/path-two/google-chrome.exe",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }

    #[test]
    fn valid_configured_executable_wins() {
        let configured = PathBuf::from("/configured/chrome");
        let cfg = BrowserConfig {
            executable: Some(configured.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let checks = Cell::new(0);

        let located = locate_executable_with(
            &cfg,
            BrowserPlatform::Windows,
            BrowserPurpose::ChromeOnly,
            &windows_environment(),
            |path| {
                checks.set(checks.get() + 1);
                path == configured
            },
        )
        .unwrap();

        assert_eq!(located, configured);
        assert_eq!(checks.get(), 1);
    }

    #[test]
    fn locate_managed_executable_accepts_real_configured_file() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("configured-chrome");
        std::fs::write(&executable, "").unwrap();
        let cfg = BrowserConfig {
            executable: Some(executable.to_string_lossy().into_owned()),
            ..Default::default()
        };

        assert_eq!(locate_managed_executable(&cfg).unwrap(), executable);
    }

    #[test]
    fn missing_configured_executable_falls_back_to_platform_candidate() {
        let cfg = BrowserConfig {
            executable: Some("/configured/missing-chrome".into()),
            ..Default::default()
        };
        let expected = PathBuf::from("/path-two/chrome.exe");

        let located = locate_executable_with(
            &cfg,
            BrowserPlatform::Windows,
            BrowserPurpose::ChromeOnly,
            &windows_environment(),
            |path| path == expected,
        )
        .unwrap();

        assert_eq!(located, expected);
    }

    #[test]
    fn injected_windows_environment_yields_exact_chrome_order() {
        assert_eq!(
            platform_candidates(
                BrowserPlatform::Windows,
                BrowserPurpose::ChromeOnly,
                &windows_environment(),
            ),
            windows_chrome_candidates(),
        );
    }

    #[test]
    fn managed_windows_candidates_begin_with_chrome_and_then_add_edge() {
        let env = windows_environment();
        let chrome =
            platform_candidates(BrowserPlatform::Windows, BrowserPurpose::ChromeOnly, &env);
        let managed = platform_candidates(BrowserPlatform::Windows, BrowserPurpose::Managed, &env);
        let edge: Vec<PathBuf> = [
            "/program-files/Microsoft/Edge/Application/msedge.exe",
            "/program-files-x86/Microsoft/Edge/Application/msedge.exe",
            "/path-one/msedge.exe",
            "/path-two/msedge.exe",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();

        assert_eq!(&managed[..chrome.len()], chrome.as_slice());
        assert_eq!(&managed[chrome.len()..], edge.as_slice());
        assert!(chrome.iter().all(|path| !path.ends_with("msedge.exe")));
    }

    #[test]
    fn macos_managed_candidates_preserve_existing_order() {
        assert_eq!(
            platform_candidates(
                BrowserPlatform::Macos,
                BrowserPurpose::Managed,
                &BrowserEnvironment::default(),
            ),
            [
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                "/Applications/Chromium.app/Contents/MacOS/Chromium",
                "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn linux_managed_candidates_preserve_existing_name_order() {
        let env = BrowserEnvironment {
            path: Some(std::env::join_paths([PathBuf::from("/usr/bin")]).unwrap()),
            ..Default::default()
        };

        assert_eq!(
            platform_candidates(BrowserPlatform::Linux, BrowserPurpose::Managed, &env),
            [
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
                "/usr/bin/microsoft-edge",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn platform_candidates_deduplicate_without_reordering() {
        let install_dir = PathBuf::from("/same/Google/Chrome/Application");
        let env = BrowserEnvironment {
            local_app_data: Some(OsString::from("/same")),
            program_files: Some(OsString::from("/same")),
            program_files_x86: None,
            path: Some(std::env::join_paths([install_dir, PathBuf::from("/bin")]).unwrap()),
        };

        assert_eq!(
            platform_candidates(BrowserPlatform::Windows, BrowserPurpose::ChromeOnly, &env,),
            [
                "/same/Google/Chrome/Application/chrome.exe",
                "/bin/chrome.exe",
                "/same/Google/Chrome/Application/google-chrome.exe",
                "/bin/google-chrome.exe",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn missing_error_names_config_purpose_and_every_candidate() {
        let cfg = BrowserConfig {
            executable: Some("/configured/missing-chrome".into()),
            ..Default::default()
        };
        let candidates = windows_chrome_candidates();

        let error = locate_executable_with(
            &cfg,
            BrowserPlatform::Windows,
            BrowserPurpose::ChromeOnly,
            &windows_environment(),
            |_path: &Path| false,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("browser.executable"), "{error}");
        assert!(error.contains("Google Chrome"), "{error}");
        assert!(error.contains("/configured/missing-chrome"), "{error}");
        assert!(error.contains("chrome.exe"), "{error}");
        assert!(error.contains("google-chrome.exe"), "{error}");
        for candidate in candidates {
            assert!(
                error.contains(&candidate.to_string_lossy().into_owned()),
                "missing {candidate:?} from {error}",
            );
        }
    }

    #[test]
    fn managed_missing_error_uses_managed_browser_label() {
        let error = locate_executable_with(
            &BrowserConfig::default(),
            BrowserPlatform::Windows,
            BrowserPurpose::Managed,
            &windows_environment(),
            |_path| false,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("Chrome/Chromium/Edge"), "{error}");
    }

    #[test]
    fn windows_extension_launches_located_chrome_directly() {
        let cfg = BrowserConfig {
            executable: Some("configured-chrome.exe".into()),
            ..Default::default()
        };

        let specs = extension_launch_specs_with(
            &cfg,
            BrowserPlatform::Windows,
            &windows_environment(),
            "chrome-extension://zode/popup.html",
            |path| path == Path::new("configured-chrome.exe"),
        )
        .unwrap();

        assert_eq!(
            specs,
            vec![LaunchSpec {
                program: PathBuf::from("configured-chrome.exe"),
                args: vec![OsString::from("chrome-extension://zode/popup.html")],
            }]
        );
        assert_ne!(specs[0].program, PathBuf::from("cmd"));
        assert!(!specs[0]
            .args
            .iter()
            .any(|arg| matches!(arg.to_str(), Some("/C" | "start"))));
    }

    #[test]
    fn macos_extension_launch_preserves_existing_fallback_order() {
        let url = "chrome-extension://zode/popup.html";

        let specs = extension_launch_specs_with(
            &BrowserConfig::default(),
            BrowserPlatform::Macos,
            &BrowserEnvironment::default(),
            url,
            |_path| false,
        )
        .unwrap();

        assert_eq!(
            specs,
            vec![
                LaunchSpec {
                    program: PathBuf::from("open"),
                    args: vec![
                        OsString::from("-a"),
                        OsString::from("Google Chrome"),
                        OsString::from(url),
                    ],
                },
                LaunchSpec {
                    program: PathBuf::from("open"),
                    args: vec![OsString::from(url)],
                },
            ]
        );
    }

    #[test]
    fn linux_extension_launch_preserves_existing_fallback_order() {
        let url = "chrome-extension://zode/popup.html";

        let specs = extension_launch_specs_with(
            &BrowserConfig::default(),
            BrowserPlatform::Linux,
            &BrowserEnvironment::default(),
            url,
            |_path| false,
        )
        .unwrap();

        assert_eq!(
            specs,
            ["google-chrome", "chromium", "chromium-browser", "xdg-open"]
                .into_iter()
                .map(|program| LaunchSpec {
                    program: PathBuf::from(program),
                    args: vec![OsString::from(url)],
                })
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn launch_specs_falls_back_in_exact_order() {
        let attempts = RefCell::new(Vec::new());
        let specs = ["first", "second"]
            .into_iter()
            .map(|program| LaunchSpec {
                program: PathBuf::from(program),
                args: Vec::new(),
            })
            .collect();

        let result = launch_specs_with(specs, |spec| {
            attempts.borrow_mut().push(spec.program.clone());
            std::future::ready(if spec.program == Path::new("first") {
                Err(io::Error::other("first failed"))
            } else {
                Ok(())
            })
        })
        .await;

        assert_eq!(result, Ok(()));
        assert_eq!(
            attempts.into_inner(),
            [PathBuf::from("first"), PathBuf::from("second")]
        );
    }

    #[tokio::test]
    async fn launch_specs_reports_every_attempted_program_and_error() {
        let specs = ["first", "second"]
            .into_iter()
            .map(|program| LaunchSpec {
                program: PathBuf::from(program),
                args: Vec::new(),
            })
            .collect();

        let error = launch_specs_with(specs, |spec| {
            let message = format!("{} failed", spec.program.display());
            std::future::ready(Err(io::Error::other(message)))
        })
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("first"), "{error}");
        assert!(error.contains("first failed"), "{error}");
        assert!(error.contains("second"), "{error}");
        assert!(error.contains("second failed"), "{error}");
    }

    #[tokio::test]
    async fn launch_specs_stops_after_first_success() {
        let attempts = Cell::new(0);
        let specs = ["first", "must-not-run"]
            .into_iter()
            .map(|program| LaunchSpec {
                program: PathBuf::from(program),
                args: Vec::new(),
            })
            .collect();

        let result = launch_specs_with(specs, |_spec| {
            attempts.set(attempts.get() + 1);
            std::future::ready(Ok(()))
        })
        .await;

        assert_eq!(result, Ok(()));
        assert_eq!(attempts.get(), 1);
    }
}
