#!/usr/bin/env bash
# Build and package the native Zode desktop application for one release target.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MACOS_ICON="$ROOT/assets/brand/zode.icns"
WINDOWS_ICON="$ROOT/assets/brand/zode.ico"
LINUX_ICON="$ROOT/assets/brand/zode-512.png"
MACOS_PLIST_TEMPLATE="$ROOT/packaging/macos/Info.plist.in"
WINDOWS_WIX_TEMPLATE="$ROOT/packaging/windows/zode-app.wxs.in"
LINUX_DESKTOP_FILE="$ROOT/packaging/linux/dev.zseven.zode.desktop"

UPGRADE_CODE="6122FC9D-4A04-4B06-906A-05FC421B6D64"
LINUXDEPLOY_VERSION="1-alpha-20251107-1"
LINUXDEPLOY_X86_64_SHA256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
LINUXDEPLOY_AARCH64_SHA256="620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff"

usage() {
  cat <<'EOF'
Usage: build-app-release.sh --target <triple> --out <dir> --version <semver> [--dry-run]

Builds zode-app and creates native release artifacts for supported macOS,
Windows, and Linux desktop targets. Windows artifacts are unsigned. macOS
artifacts use an explicit ad-hoc signature unless a future release workflow
adds a separate identity-based signing step.
EOF
}

die() {
  echo "build-app-release: $*" >&2
  usage >&2
  exit 2
}

need_value() {
  local option="$1"
  local value="${2:-}"
  [[ -n "$value" && "$value" != --* ]] || die "$option requires a value"
}

need_cmd() {
  local command_name="$1"
  command -v "$command_name" >/dev/null 2>&1 \
    || die "required command not found: $command_name"
}

need_file() {
  local path="$1"
  local label="$2"
  [[ -s "$path" ]] || die "missing $label: $path"
}

refuse_existing() {
  local path="$1"
  [[ ! -e "$path" ]] || die "refusing to overwrite existing artifact: $path"
}

TARGET=""
OUT=""
VERSION_INPUT=""
DRY_RUN=0

while (($#)); do
  case "$1" in
    --target)
      need_value "$1" "${2:-}"
      TARGET="$2"
      shift 2
      ;;
    --out)
      need_value "$1" "${2:-}"
      OUT="$2"
      shift 2
      ;;
    --version)
      need_value "$1" "${2:-}"
      VERSION_INPUT="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "$TARGET" ]] || die "missing required --target"
[[ -n "$OUT" ]] || die "missing required --out"
[[ -n "$VERSION_INPUT" ]] || die "missing required --version"

VERSION="${VERSION_INPUT#v}"
SEMVER_PATTERN='^([0-9]+)\.([0-9]+)\.([0-9]+)(-([0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*))?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
if [[ ! "$VERSION" =~ $SEMVER_PATTERN ]]; then
  die "invalid semantic version: $VERSION_INPUT"
fi
VERSION_MAJOR="${BASH_REMATCH[1]}"
VERSION_MINOR="${BASH_REMATCH[2]}"
VERSION_PATCH="${BASH_REMATCH[3]}"
VERSION_PRERELEASE="${BASH_REMATCH[5]:-}"
for numeric_identifier in "$VERSION_MAJOR" "$VERSION_MINOR" "$VERSION_PATCH"; do
  if ((${#numeric_identifier} > 1)) && [[ "$numeric_identifier" == 0* ]]; then
    die "invalid semantic version: $VERSION_INPUT"
  fi
done
if [[ -n "$VERSION_PRERELEASE" ]]; then
  IFS='.' read -r -a prerelease_identifiers <<<"$VERSION_PRERELEASE"
  for prerelease_identifier in "${prerelease_identifiers[@]}"; do
    if [[ "$prerelease_identifier" =~ ^[0-9]+$ ]] \
      && ((${#prerelease_identifier} > 1)) \
      && [[ "$prerelease_identifier" == 0* ]]; then
      die "invalid semantic version: $VERSION_INPUT"
    fi
  done
fi
BUNDLE_VERSION="$((10#$VERSION_MAJOR)).$((10#$VERSION_MINOR)).$((10#$VERSION_PATCH))"
MSI_VERSION="$BUNDLE_VERSION"

case "$TARGET" in
  aarch64-apple-darwin|x86_64-apple-darwin)
    PLATFORM="macos"
    ;;
  aarch64-pc-windows-msvc|x86_64-pc-windows-msvc)
    PLATFORM="windows"
    if ((10#$VERSION_MAJOR > 255 || 10#$VERSION_MINOR > 255 || 10#$VERSION_PATCH > 65535)); then
      die "semantic version $VERSION cannot be represented as an MSI version"
    fi
    ;;
  aarch64-unknown-linux-gnu|x86_64-unknown-linux-gnu)
    PLATFORM="linux"
    ;;
  *)
    die "unsupported desktop target: $TARGET"
    ;;
esac

PREFIX="zode-desktop-$VERSION-$TARGET"

print_plan() {
  echo "platform=$PLATFORM"
  echo "target=$TARGET"
  echo "version=$VERSION"
  case "$PLATFORM" in
    macos)
      echo "bundle_version=$BUNDLE_VERSION"
      echo "signing=ad-hoc"
      echo "artifact=$OUT/Zode.app"
      echo "artifact=$OUT/$PREFIX.tar.gz"
      ;;
    windows)
      echo "msi_version=$MSI_VERSION"
      echo "upgrade_code=$UPGRADE_CODE"
      echo "signing=unsigned"
      echo "artifact=$OUT/$PREFIX.zip"
      echo "artifact=$OUT/$PREFIX.msi"
      ;;
    linux)
      local sha256
      if [[ "$TARGET" == x86_64-* ]]; then
        sha256="$LINUXDEPLOY_X86_64_SHA256"
      else
        sha256="$LINUXDEPLOY_AARCH64_SHA256"
      fi
      echo "linuxdeploy_version=$LINUXDEPLOY_VERSION"
      echo "linuxdeploy_sha256=$sha256"
      echo "artifact=$OUT/$PREFIX.AppImage"
      echo "artifact=$OUT/$PREFIX.tar.gz"
      ;;
  esac
}

if ((DRY_RUN)); then
  print_plan
  exit 0
fi

resolve_binary() {
  local executable_name="zode-app"
  if [[ "$PLATFORM" == windows ]]; then
    executable_name="zode-app.exe"
  fi

  if [[ -n "${ZODE_APP_BINARY:-}" ]]; then
    [[ -s "$ZODE_APP_BINARY" ]] || die "ZODE_APP_BINARY does not exist: $ZODE_APP_BINARY"
    printf '%s\n' "$ZODE_APP_BINARY"
    return
  fi

  need_cmd cargo
  local -a cargo_command=(cargo)
  if [[ -n "${ZODE_CARGO_TOOLCHAIN:-}" ]]; then
    cargo_command=(cargo "+$ZODE_CARGO_TOOLCHAIN")
  elif cargo +1.94 --version >/dev/null 2>&1; then
    cargo_command=(cargo +1.94)
  fi
  (
    cd "$ROOT"
    "${cargo_command[@]}" build --locked --release -p zode-app --target "$TARGET"
  )

  local target_dir="${CARGO_TARGET_DIR:-$ROOT/target}"
  if [[ "$target_dir" != /* ]]; then
    target_dir="$ROOT/$target_dir"
  fi
  local binary="$target_dir/$TARGET/release/$executable_name"
  [[ -s "$binary" ]] || die "cargo build completed without expected binary: $binary"
  printf '%s\n' "$binary"
}

sed_replacement() {
  printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'
}

render_macos_plist() {
  local destination="$1"
  local bundle_version release_version
  bundle_version="$(sed_replacement "$BUNDLE_VERSION")"
  release_version="$(sed_replacement "$VERSION")"
  sed \
    -e "s|@@BUNDLE_VERSION@@|$bundle_version|g" \
    -e "s|@@RELEASE_VERSION@@|$release_version|g" \
    "$MACOS_PLIST_TEMPLATE" >"$destination"
}

render_wix_source() {
  local destination="$1"
  local binary_path="$2"
  local icon_path="$3"
  local version upgrade_code binary icon
  version="$(sed_replacement "$MSI_VERSION")"
  upgrade_code="$(sed_replacement "$UPGRADE_CODE")"
  binary="$(sed_replacement "$binary_path")"
  icon="$(sed_replacement "$icon_path")"
  sed \
    -e "s|@@VERSION@@|$version|g" \
    -e "s|@@UPGRADE_CODE@@|$upgrade_code|g" \
    -e "s|@@BINARY_PATH@@|$binary|g" \
    -e "s|@@ICON_PATH@@|$icon|g" \
    "$WINDOWS_WIX_TEMPLATE" >"$destination"
}

audit_macos_dependencies() {
  local binary="$1"
  local report="$2"
  need_cmd otool
  otool -L "$binary" >"$report" \
    || die "failed to inspect macOS dependencies: $binary"
  if grep -Fq 'not found' "$report"; then
    die "macOS dependency audit found an unresolved library; see $report"
  fi

  local dependency
  while IFS= read -r dependency; do
    case "$dependency" in
      /System/Library/*|/usr/lib/*)
        ;;
      @*)
        die "macOS bundle has an unbundled relative dependency: $dependency"
        ;;
      /*)
        die "macOS bundle has a non-system absolute dependency: $dependency"
        ;;
    esac
  done < <(awk 'NR > 1 { print $1 }' "$report")
}

package_macos() {
  [[ "$(uname -s)" == Darwin ]] \
    || die "macOS packaging must run on a macOS host"
  need_cmd codesign
  need_cmd lipo
  need_cmd plutil
  need_cmd tar
  need_file "$MACOS_ICON" "macOS .icns icon"
  need_file "$MACOS_PLIST_TEMPLATE" "macOS Info.plist template"

  local binary app archive destination archs expected_arch dependency_report
  binary="$(resolve_binary)"
  app="$OUT/Zode.app"
  archive="$OUT/$PREFIX.tar.gz"
  destination="$app/Contents/MacOS/zode-app"
  dependency_report="$app/Contents/Resources/zode-app.dependencies.txt"
  refuse_existing "$app"
  refuse_existing "$archive"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$binary" "$destination"
  chmod 755 "$destination"
  cp "$MACOS_ICON" "$app/Contents/Resources/Zode.icns"
  render_macos_plist "$app/Contents/Info.plist"
  plutil -lint "$app/Contents/Info.plist" >/dev/null \
    || die "generated Info.plist is invalid"

  archs="$(lipo -archs "$destination")" \
    || die "failed to inspect macOS executable architecture"
  if [[ "$TARGET" == aarch64-* ]]; then
    expected_arch="arm64"
    [[ " $archs " == *" arm64 "* || " $archs " == *" arm64e "* ]] \
      || die "macOS executable does not contain $expected_arch: $archs"
  else
    expected_arch="x86_64"
    [[ " $archs " == *" $expected_arch "* ]] \
      || die "macOS executable does not contain $expected_arch: $archs"
  fi

  audit_macos_dependencies "$destination" "$dependency_report"
  codesign --force --deep --sign - --timestamp=none "$app" >/dev/null \
    || die "ad-hoc codesign failed for $app"
  codesign --verify --deep --strict --verbose=2 "$app" \
    || die "ad-hoc codesign verification failed for $app"
  COPYFILE_DISABLE=1 tar -C "$OUT" -czf "$archive" Zode.app
  [[ -s "$archive" ]] || die "macOS archive was not created: $archive"
}

package_windows() {
  need_cmd powershell.exe
  need_cmd cygpath
  need_file "$WINDOWS_ICON" "Windows .ico icon"
  need_file "$WINDOWS_WIX_TEMPLATE" "WiX v4 source template"
  need_file "$ROOT/packaging/windows/UNSIGNED.txt" "Windows unsigned notice"

  local wix_command
  if command -v wix >/dev/null 2>&1; then
    wix_command="wix"
  elif command -v wix.exe >/dev/null 2>&1; then
    wix_command="wix.exe"
  else
    die "required command not found: wix (WiX Toolset v4)"
  fi

  local binary portable zip_path msi_path wxs_path wix_arch
  local portable_windows zip_windows msi_windows wxs_windows binary_windows icon_windows
  binary="$(resolve_binary)"
  portable="$OUT/$PREFIX-portable"
  zip_path="$OUT/$PREFIX.zip"
  msi_path="$OUT/$PREFIX.msi"
  wxs_path="$OUT/$PREFIX.wxs"
  refuse_existing "$portable"
  refuse_existing "$zip_path"
  refuse_existing "$msi_path"
  refuse_existing "$wxs_path"
  mkdir -p "$portable"
  cp "$binary" "$portable/zode-app.exe"
  cp "$WINDOWS_ICON" "$portable/zode.ico"
  cp "$ROOT/LICENSE" "$portable/LICENSE"
  cp "$ROOT/packaging/windows/UNSIGNED.txt" "$portable/UNSIGNED.txt"

  portable_windows="$(cygpath -w "$portable")"
  zip_windows="$(cygpath -w "$zip_path")"
  msi_windows="$(cygpath -w "$msi_path")"
  wxs_windows="$(cygpath -w "$wxs_path")"
  binary_windows="$(cygpath -w "$portable/zode-app.exe")"
  icon_windows="$(cygpath -w "$WINDOWS_ICON")"
  render_wix_source "$wxs_path" "$binary_windows" "$icon_windows"

  PORTABLE_WINDOWS="$portable_windows" ZIP_WINDOWS="$zip_windows" \
    powershell.exe -NoProfile -NonInteractive -Command \
      'Compress-Archive -Path "$env:PORTABLE_WINDOWS\*" -DestinationPath "$env:ZIP_WINDOWS"' \
    || die "failed to create Windows portable zip"
  if [[ "$TARGET" == aarch64-* ]]; then
    wix_arch="arm64"
  else
    wix_arch="x64"
  fi
  "$wix_command" build -arch "$wix_arch" -o "$msi_windows" "$wxs_windows" \
    || die "WiX v4 failed to create MSI"
  [[ -s "$zip_path" ]] || die "Windows portable zip was not created: $zip_path"
  [[ -s "$msi_path" ]] || die "Windows MSI was not created: $msi_path"
  echo "Windows artifacts are unsigned: $zip_path, $msi_path"
}

verify_sha256() {
  local file="$1"
  local expected="$2"
  printf '%s  %s\n' "$expected" "$file" | sha256sum -c - >/dev/null \
    || die "checksum mismatch for downloaded linuxdeploy: $file"
}

resolve_linuxdeploy() {
  need_cmd curl
  need_cmd sha256sum
  local arch sha256 tool_dir tool url download
  if [[ "$TARGET" == x86_64-* ]]; then
    arch="x86_64"
    sha256="$LINUXDEPLOY_X86_64_SHA256"
  else
    arch="aarch64"
    sha256="$LINUXDEPLOY_AARCH64_SHA256"
  fi
  tool_dir="${ZODE_PACKAGING_TOOL_DIR:-$ROOT/target/packaging-tools}"
  tool="$tool_dir/linuxdeploy-$LINUXDEPLOY_VERSION-$arch.AppImage"
  url="https://github.com/linuxdeploy/linuxdeploy/releases/download/$LINUXDEPLOY_VERSION/linuxdeploy-$arch.AppImage"
  mkdir -p "$tool_dir"
  if [[ -e "$tool" ]]; then
    verify_sha256 "$tool" "$sha256"
  else
    download="$tool.download.$$"
    refuse_existing "$download"
    if ! curl --fail --location --proto '=https' --tlsv1.2 --retry 3 \
      --output "$download" "$url"; then
      rm -f "$download"
      die "failed to download pinned linuxdeploy release: $url"
    fi
    verify_sha256 "$download" "$sha256"
    chmod 755 "$download"
    mv "$download" "$tool"
  fi
  printf '%s\n' "$tool"
}

audit_linux_dependencies() {
  local binary="$1"
  local report="$2"
  need_cmd ldd
  ldd "$binary" >"$report" 2>&1 \
    || die "failed to inspect Linux dependencies; see $report"
  if grep -Fq 'not found' "$report"; then
    die "Linux dependency audit found an unresolved library; see $report"
  fi
}

package_linux() {
  [[ "$(uname -s)" == Linux ]] \
    || die "Linux packaging must run on a Linux host"
  need_cmd tar
  need_file "$LINUX_ICON" "Linux PNG icon"
  need_file "$LINUX_DESKTOP_FILE" "Linux desktop entry"

  local binary linuxdeploy appdir portable appimage archive desktop icon report
  binary="$(resolve_binary)"
  linuxdeploy="$(resolve_linuxdeploy)"
  appdir="$OUT/$PREFIX.AppDir"
  portable="$OUT/$PREFIX-portable"
  appimage="$OUT/$PREFIX.AppImage"
  archive="$OUT/$PREFIX.tar.gz"
  desktop="$OUT/dev.zseven.zode.desktop"
  icon="$OUT/dev.zseven.zode.png"
  report="$portable/zode-app.dependencies.txt"
  refuse_existing "$appdir"
  refuse_existing "$portable"
  refuse_existing "$appimage"
  refuse_existing "$archive"
  refuse_existing "$desktop"
  refuse_existing "$icon"
  mkdir -p "$appdir" "$portable"
  cp "$LINUX_DESKTOP_FILE" "$desktop"
  cp "$LINUX_ICON" "$icon"
  cp "$binary" "$portable/zode-app"
  chmod 755 "$portable/zode-app"
  cp "$LINUX_DESKTOP_FILE" "$portable/dev.zseven.zode.desktop"
  cp "$LINUX_ICON" "$portable/dev.zseven.zode.png"
  cp "$ROOT/LICENSE" "$portable/LICENSE"
  audit_linux_dependencies "$portable/zode-app" "$report"

  OUTPUT="$appimage" VERSION="$VERSION" "$linuxdeploy" --appimage-extract-and-run \
    --appdir "$appdir" \
    --executable "$binary" \
    --desktop-file "$desktop" \
    --icon-file "$icon" \
    --output appimage \
    || die "linuxdeploy failed to create AppImage"
  [[ -s "$appimage" ]] || die "linuxdeploy did not create expected AppImage: $appimage"
  tar -C "$OUT" -czf "$archive" "$PREFIX-portable"
  [[ -s "$archive" ]] || die "Linux portable archive was not created: $archive"
}

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
case "$PLATFORM" in
  macos) package_macos ;;
  windows) package_windows ;;
  linux) package_linux ;;
esac

print_plan
echo "status=packaged"
