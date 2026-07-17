use std::{fs, path::Path};

#[cfg(target_os = "macos")]
use std::process::Command;

use zode_app_model::ExternalApplication;
use zode_node_protocol::WorkspaceUri;

use super::{workspace_root, ExternalApplicationService, ServiceError};

#[derive(Default)]
pub struct LocalExternalApplicationService;

impl ExternalApplicationService for LocalExternalApplicationService {
    fn installed_applications(&self) -> Result<Vec<ExternalApplication>, ServiceError> {
        Ok(scan_installed_applications(&application_roots()))
    }

    fn open_workspace(
        &self,
        workspace: &WorkspaceUri,
        application: ExternalApplication,
    ) -> Result<(), ServiceError> {
        let path = workspace_root(workspace)?.canonicalize()?;
        if !path.is_dir() {
            return Err(ServiceError::NotDirectory(path.display().to_string()));
        }
        open_workspace_with(&path, application)
    }
}

fn application_roots() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut roots = vec![
            "/Applications".into(),
            "/System/Applications".into(),
            "/System/Library/CoreServices".into(),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(std::path::PathBuf::from(home).join("Applications"));
        }
        roots
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

fn scan_installed_applications(roots: &[std::path::PathBuf]) -> Vec<ExternalApplication> {
    let mut bundle_names = Vec::new();
    for root in roots {
        collect_application_names(root, 0, &mut bundle_names);
    }
    ExternalApplication::ALL
        .into_iter()
        .filter(|application| {
            application.bundle_names().iter().any(|candidate| {
                bundle_names
                    .iter()
                    .any(|installed| installed.eq_ignore_ascii_case(candidate))
            })
        })
        .collect()
}

fn collect_application_names(path: &Path, depth: usize, names: &mut Vec<String>) {
    if depth > 2 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if path.extension().is_some_and(|extension| extension == "app") {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                names.push(stem.to_owned());
            }
            continue;
        }
        collect_application_names(&path, depth + 1, names);
    }
}

#[cfg(target_os = "macos")]
fn open_workspace_with(path: &Path, application: ExternalApplication) -> Result<(), ServiceError> {
    let stable_status = Command::new("open")
        .arg("-b")
        .arg(bundle_identifier(application))
        .arg(path)
        .status()?;
    if stable_status.success() {
        return Ok(());
    }
    for bundle_name in application.bundle_names() {
        let status = Command::new("open")
            .arg("-a")
            .arg(bundle_name)
            .arg(path)
            .status()?;
        if status.success() {
            return Ok(());
        }
    }
    Err(ServiceError::Platform(format!(
        "{} could not open the workspace",
        application.label()
    )))
}

#[cfg(not(target_os = "macos"))]
fn open_workspace_with(_path: &Path, application: ExternalApplication) -> Result<(), ServiceError> {
    Err(ServiceError::Platform(format!(
        "opening with {} is not supported on this platform",
        application.label()
    )))
}

#[cfg(target_os = "macos")]
const fn bundle_identifier(application: ExternalApplication) -> &'static str {
    match application {
        ExternalApplication::VisualStudioCode => "com.microsoft.VSCode",
        ExternalApplication::Cursor => "com.todesktop.230313mzl4w4u92",
        ExternalApplication::Zed => "dev.zed.Zed",
        ExternalApplication::Finder => "com.apple.finder",
        ExternalApplication::Terminal => "com.apple.Terminal",
        ExternalApplication::ITerm2 => "com.googlecode.iterm2",
        ExternalApplication::Warp => "dev.warp.Warp-Stable",
        ExternalApplication::Xcode => "com.apple.dt.Xcode",
        ExternalApplication::AndroidStudio => "com.google.android.studio",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::scan_installed_applications;
    use zode_app_model::ExternalApplication;

    #[test]
    fn scans_only_supported_installed_bundles_in_product_order() {
        let root = std::env::temp_dir().join(format!(
            "zode-external-applications-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("Visual Studio Code.app")).unwrap();
        fs::create_dir_all(root.join("Utilities/Terminal.app")).unwrap();
        fs::create_dir_all(root.join("Unrelated.app")).unwrap();

        assert_eq!(
            scan_installed_applications(std::slice::from_ref(&root)),
            vec![
                ExternalApplication::VisualStudioCode,
                ExternalApplication::Terminal,
            ]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recognizes_preview_and_beta_bundle_names() {
        let root = std::env::temp_dir().join(format!(
            "zode-external-application-variants-{}",
            uuid::Uuid::new_v4()
        ));
        for name in [
            "Zed Preview.app",
            "Xcode-beta.app",
            "Android Studio Preview.app",
        ] {
            fs::create_dir_all(root.join(name)).unwrap();
        }

        assert_eq!(
            scan_installed_applications(std::slice::from_ref(&root)),
            vec![
                ExternalApplication::Zed,
                ExternalApplication::Xcode,
                ExternalApplication::AndroidStudio,
            ]
        );
        let _ = fs::remove_dir_all(root);
    }
}
