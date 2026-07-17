use std::{fs, path::Path};

#[cfg(target_os = "macos")]
use std::process::Command;

use zode_app_model::{ExternalApplication, ExternalApplicationCatalog, ExternalApplicationIcon};
use zode_node_protocol::WorkspaceUri;

use super::{workspace_root, ExternalApplicationService, ServiceError};

#[derive(Default)]
pub struct LocalExternalApplicationService;

impl ExternalApplicationService for LocalExternalApplicationService {
    fn installed_applications(&self) -> Result<ExternalApplicationCatalog, ServiceError> {
        Ok(scan_installed_application_catalog(&application_roots()))
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

fn scan_installed_application_catalog(roots: &[std::path::PathBuf]) -> ExternalApplicationCatalog {
    scan_installed_application_catalog_with(roots, application_icon_png)
}

fn scan_installed_application_catalog_with(
    roots: &[std::path::PathBuf],
    mut load_icon: impl FnMut(&Path) -> Option<Vec<u8>>,
) -> ExternalApplicationCatalog {
    let bundles = installed_application_bundles(roots);
    let mut catalog = ExternalApplicationCatalog::default();
    for application in ExternalApplication::ALL {
        let Some(bundle_path) = resolve_application_bundle(&bundles, application) else {
            continue;
        };
        catalog.applications.push(application);
        if let Some(encoded_png) = load_icon(bundle_path) {
            catalog
                .icons
                .push(ExternalApplicationIcon::new(application, encoded_png));
        }
    }
    catalog
}

#[cfg(test)]
fn scan_installed_applications(roots: &[std::path::PathBuf]) -> Vec<ExternalApplication> {
    scan_installed_application_catalog(roots).applications
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstalledApplicationBundle {
    root_index: usize,
    name: String,
    path: std::path::PathBuf,
}

fn installed_application_bundles(roots: &[std::path::PathBuf]) -> Vec<InstalledApplicationBundle> {
    let mut bundles = Vec::new();
    for (root_index, root) in roots.iter().enumerate() {
        collect_application_bundles(root, 0, root_index, &mut bundles);
    }
    bundles.sort_by(|left, right| {
        left.root_index
            .cmp(&right.root_index)
            .then_with(|| left.path.cmp(&right.path))
    });
    bundles
}

fn resolve_application_bundle(
    bundles: &[InstalledApplicationBundle],
    application: ExternalApplication,
) -> Option<&Path> {
    application.bundle_names().iter().find_map(|candidate| {
        bundles
            .iter()
            .find(|bundle| bundle.name.eq_ignore_ascii_case(candidate))
            .map(|bundle| bundle.path.as_path())
    })
}

#[cfg(any(target_os = "macos", test))]
fn resolve_application_bundle_from_roots(
    roots: &[std::path::PathBuf],
    application: ExternalApplication,
) -> Option<std::path::PathBuf> {
    let bundles = installed_application_bundles(roots);
    resolve_application_bundle(&bundles, application).map(Path::to_path_buf)
}

fn collect_application_bundles(
    path: &Path,
    depth: usize,
    root_index: usize,
    bundles: &mut Vec<InstalledApplicationBundle>,
) {
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
                bundles.push(InstalledApplicationBundle {
                    root_index,
                    name: stem.to_owned(),
                    path,
                });
            }
            continue;
        }
        collect_application_bundles(&path, depth + 1, root_index, bundles);
    }
}

#[cfg(target_os = "macos")]
fn application_icon_png(bundle_path: &Path) -> Option<Vec<u8>> {
    let icon_path = application_icon_path(bundle_path)?;
    let output_path = std::env::temp_dir().join(format!(
        "zode-external-application-icon-{}.png",
        uuid::Uuid::new_v4()
    ));
    let output = Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "-z", "64", "64"])
        .arg(icon_path)
        .arg("--out")
        .arg(&output_path)
        .output()
        .ok()?;
    let encoded = output
        .status
        .success()
        .then(|| fs::read(&output_path).ok())
        .flatten();
    let _ = fs::remove_file(output_path);
    encoded.filter(|bytes| bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
}

#[cfg(target_os = "macos")]
fn application_icon_path(bundle_path: &Path) -> Option<std::path::PathBuf> {
    let info_path = bundle_path.join("Contents/Info.plist");
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIconFile", "raw", "-o", "-"])
        .arg(info_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return fallback_icon_path(bundle_path);
    }
    let icon_name = String::from_utf8(output.stdout).ok()?;
    let icon_name = icon_name.trim();
    if icon_name.is_empty() {
        return fallback_icon_path(bundle_path);
    }
    let resources = bundle_path.join("Contents/Resources");
    let direct = resources.join(icon_name);
    if direct.is_file() {
        return Some(direct);
    }
    if direct.extension().is_none() {
        let with_extension = resources.join(format!("{icon_name}.icns"));
        if with_extension.is_file() {
            return Some(with_extension);
        }
    }
    fallback_icon_path(bundle_path)
}

#[cfg(target_os = "macos")]
fn fallback_icon_path(bundle_path: &Path) -> Option<std::path::PathBuf> {
    let resources = bundle_path.join("Contents/Resources");
    let bundle_name = bundle_path.file_stem()?.to_str()?;
    let mut icons = fs::read_dir(resources)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("icns"))
        })
        .collect::<Vec<_>>();
    icons.sort();
    icons
        .iter()
        .find(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.eq_ignore_ascii_case(bundle_name))
        })
        .cloned()
        .or_else(|| (icons.len() == 1).then(|| icons.remove(0)))
}

#[cfg(not(target_os = "macos"))]
fn application_icon_png(_bundle_path: &Path) -> Option<Vec<u8>> {
    None
}

#[cfg(target_os = "macos")]
fn open_workspace_with(path: &Path, application: ExternalApplication) -> Result<(), ServiceError> {
    if let Some(bundle_path) =
        resolve_application_bundle_from_roots(&application_roots(), application)
    {
        let resolved_status = Command::new("open")
            .arg("-a")
            .arg(bundle_path)
            .arg(path)
            .status()?;
        if resolved_status.success() {
            return Ok(());
        }
    }
    let bundle_id_status = Command::new("open")
        .arg("-b")
        .arg(bundle_identifier(application))
        .arg(path)
        .status()?;
    if bundle_id_status.success() {
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

    use super::{
        resolve_application_bundle_from_roots, scan_installed_application_catalog,
        scan_installed_application_catalog_with, scan_installed_applications,
    };
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

    #[test]
    fn stable_bundle_and_earlier_root_drive_both_selection_and_catalog_icon() {
        let fixture = std::env::temp_dir().join(format!(
            "zode-external-application-resolution-{}",
            uuid::Uuid::new_v4()
        ));
        let first_root = fixture.join("first");
        let second_root = fixture.join("second");
        let preview = first_root.join("Zed Preview.app");
        let first_stable = first_root.join("Zed.app");
        let second_stable = second_root.join("Zed.app");
        for (bundle, marker) in [
            (&preview, b"preview".as_slice()),
            (&first_stable, b"first-stable".as_slice()),
            (&second_stable, b"second-stable".as_slice()),
        ] {
            fs::create_dir_all(bundle).unwrap();
            fs::write(bundle.join("icon-marker"), marker).unwrap();
        }
        let roots = vec![first_root, second_root];

        assert_eq!(
            resolve_application_bundle_from_roots(&roots, ExternalApplication::Zed),
            Some(first_stable.clone()),
            "stable must beat Preview, then the earlier configured root must win"
        );
        let catalog = scan_installed_application_catalog_with(&roots, |bundle| {
            fs::read(bundle.join("icon-marker")).ok()
        });
        assert_eq!(catalog.applications, vec![ExternalApplication::Zed]);
        assert_eq!(catalog.icons.len(), 1);
        assert_eq!(catalog.icons[0].application, ExternalApplication::Zed);
        assert_eq!(catalog.icons[0].encoded_png(), b"first-stable");

        let _ = fs::remove_dir_all(fixture);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn catalog_loads_the_real_bundle_icon_as_a_small_png() {
        let root = std::env::temp_dir().join(format!(
            "zode-external-application-icon-{}",
            uuid::Uuid::new_v4()
        ));
        let bundle = root.join("Zed.app");
        let resources = bundle.join("Contents/Resources");
        fs::create_dir_all(&resources).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleIconFile</key><string>ZedIcon.png</string></dict></plist>"#,
        )
        .unwrap();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([45, 75, 255, 255]))
            .save(resources.join("ZedIcon.png"))
            .unwrap();

        let catalog = scan_installed_application_catalog(std::slice::from_ref(&root));
        assert_eq!(catalog.applications, vec![ExternalApplication::Zed]);
        assert_eq!(catalog.icons.len(), 1);
        assert_eq!(catalog.icons[0].application, ExternalApplication::Zed);
        assert!(catalog.icons[0]
            .encoded_png()
            .starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoded = image::load_from_memory(catalog.icons[0].encoded_png()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 64));
        let _ = fs::remove_dir_all(root);
    }
}
