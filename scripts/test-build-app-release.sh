#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGER="$ROOT/scripts/build-app-release.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/zode-app-package-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  [[ "$haystack" == *"$needle"* ]] || fail "expected output to contain: $needle"
}

run_fails() {
  local output_file="$TMP/failure-output"
  if "$@" >"$output_file" 2>&1; then
    fail "command unexpectedly succeeded: $*"
  fi
  cat "$output_file"
}

test_required_arguments() {
  local output
  output="$(run_fails "$PACKAGER" --target aarch64-apple-darwin --out "$TMP/out")"
  assert_contains "$output" "missing required --version"
  assert_contains "$output" "Usage:"
}

test_unknown_target() {
  local output
  output="$(run_fails "$PACKAGER" --target wasm32-unknown-unknown --out "$TMP/out" --version 1.2.3 --dry-run)"
  assert_contains "$output" "unsupported desktop target: wasm32-unknown-unknown"
}

test_invalid_version() {
  local output
  output="$(run_fails "$PACKAGER" --target aarch64-apple-darwin --out "$TMP/out" --version release-latest --dry-run)"
  assert_contains "$output" "invalid semantic version: release-latest"
  output="$(run_fails "$PACKAGER" --target aarch64-apple-darwin --out "$TMP/out" --version 01.2.3 --dry-run)"
  assert_contains "$output" "invalid semantic version: 01.2.3"
  output="$(run_fails "$PACKAGER" --target aarch64-apple-darwin --out "$TMP/out" --version 1.2.3-beta.04 --dry-run)"
  assert_contains "$output" "invalid semantic version: 1.2.3-beta.04"
}

test_unknown_argument() {
  local output
  output="$(run_fails "$PACKAGER" --target aarch64-apple-darwin --out "$TMP/out" --version 1.2.3 --wat)"
  assert_contains "$output" "unknown argument: --wat"
}

test_macos_dry_run_contract() {
  local output
  output="$("$PACKAGER" --target aarch64-apple-darwin --out "$TMP/macos" --version 1.2.3-beta.4 --dry-run)"
  assert_contains "$output" "platform=macos"
  assert_contains "$output" "target=aarch64-apple-darwin"
  assert_contains "$output" "version=1.2.3-beta.4"
  assert_contains "$output" "bundle_version=1.2.3"
  assert_contains "$output" "signing=ad-hoc"
  assert_contains "$output" "artifact=$TMP/macos/Zode.app"
  assert_contains "$output" "artifact=$TMP/macos/zode-desktop-1.2.3-beta.4-aarch64-apple-darwin.tar.gz"
}

test_windows_dry_run_contract() {
  local output
  output="$("$PACKAGER" --target x86_64-pc-windows-msvc --out "$TMP/windows" --version v9.8.7-rc.2 --dry-run)"
  assert_contains "$output" "platform=windows"
  assert_contains "$output" "version=9.8.7-rc.2"
  assert_contains "$output" "msi_version=9.8.7"
  assert_contains "$output" "upgrade_code=6122FC9D-4A04-4B06-906A-05FC421B6D64"
  assert_contains "$output" "signing=unsigned"
  assert_contains "$output" "artifact=$TMP/windows/zode-desktop-9.8.7-rc.2-x86_64-pc-windows-msvc.zip"
  assert_contains "$output" "artifact=$TMP/windows/zode-desktop-9.8.7-rc.2-x86_64-pc-windows-msvc.msi"
}

test_windows_version_limits() {
  local output
  output="$(run_fails "$PACKAGER" --target x86_64-pc-windows-msvc --out "$TMP/windows" --version 256.0.0 --dry-run)"
  assert_contains "$output" "cannot be represented as an MSI version"
}

test_linux_dry_run_contract() {
  local output
  output="$("$PACKAGER" --target aarch64-unknown-linux-gnu --out "$TMP/linux" --version 2.0.1 --dry-run)"
  assert_contains "$output" "platform=linux"
  assert_contains "$output" "linuxdeploy_version=1-alpha-20251107-1"
  assert_contains "$output" "linuxdeploy_sha256=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff"
  assert_contains "$output" "artifact=$TMP/linux/zode-desktop-2.0.1-aarch64-unknown-linux-gnu.AppImage"
  assert_contains "$output" "artifact=$TMP/linux/zode-desktop-2.0.1-aarch64-unknown-linux-gnu.tar.gz"
}

test_missing_platform_tool_is_explicit() {
  local binary="$TMP/zode-app.exe"
  local output
  printf 'fake binary\n' >"$binary"
  chmod +x "$binary"
  output="$(run_fails env PATH=/usr/bin:/bin ZODE_APP_BINARY="$binary" \
    "$PACKAGER" --target x86_64-pc-windows-msvc --out "$TMP/no-tools" --version 1.0.0)"
  assert_contains "$output" "required command not found:"
}

test_packaging_assets_are_complete() {
  [[ -s "$ROOT/assets/brand/zode.icns" ]] || fail "missing macOS icon"
  [[ -s "$ROOT/assets/brand/zode.ico" ]] || fail "missing Windows icon"
  [[ -s "$ROOT/packaging/macos/Info.plist.in" ]] || fail "missing Info.plist template"
  [[ -s "$ROOT/packaging/windows/zode-app.wxs.in" ]] || fail "missing WiX v4 template"
  [[ -s "$ROOT/packaging/linux/dev.zseven.zode.desktop" ]] || fail "missing desktop entry"
  grep -Fq 'xmlns="http://wixtoolset.org/schemas/v4/wxs"' "$ROOT/packaging/windows/zode-app.wxs.in" \
    || fail "WiX template is not v4"
  grep -Fq 'UpgradeCode="@@UPGRADE_CODE@@"' "$ROOT/packaging/windows/zode-app.wxs.in" \
    || fail "WiX template does not expose the stable UpgradeCode"
  grep -Fq 'Icon=dev.zseven.zode' "$ROOT/packaging/linux/dev.zseven.zode.desktop" \
    || fail "desktop entry does not reference the packaged icon"
}

test_windows_executable_is_gui_subsystem() {
  grep -Fq '#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]' \
    "$ROOT/crates/zode-app/src/main.rs" \
    || fail "zode-app does not opt into the Windows GUI subsystem"
}

test_windows_executable_embeds_brand_icon() {
  grep -Fq 'build = "build.rs"' "$ROOT/crates/zode-app/Cargo.toml" \
    || fail "zode-app does not declare its Windows resource build script"
  grep -Fq 'winresource = "0.1.31"' "$ROOT/crates/zode-app/Cargo.toml" \
    || fail "zode-app does not pin the Windows resource builder"
  grep -Fq '../../assets/brand/zode.ico' "$ROOT/crates/zode-app/build.rs" \
    || fail "Windows executable resource does not use the Zode icon"
  grep -Fq 'CARGO_CFG_TARGET_OS' "$ROOT/crates/zode-app/build.rs" \
    || fail "Windows resource build is not target-aware"
}

test_macos_native_bundle_path() {
  [[ "$(uname -s)" == Darwin ]] || return 0
  local out="$TMP/native-macos"
  local archive="$out/zode-desktop-3.4.5-beta.6-aarch64-apple-darwin.tar.gz"
  ZODE_APP_BINARY=/usr/bin/true "$PACKAGER" \
    --target aarch64-apple-darwin \
    --out "$out" \
    --version 3.4.5-beta.6 >/dev/null
  [[ -x "$out/Zode.app/Contents/MacOS/zode-app" ]] || fail "bundle executable is missing"
  [[ -s "$out/Zode.app/Contents/Resources/Zode.icns" ]] || fail "bundle icon is missing"
  [[ -s "$out/Zode.app/Contents/Resources/zode-app.dependencies.txt" ]] \
    || fail "dependency audit is missing"
  [[ -s "$archive" ]] || fail "macOS archive is missing"
  grep -Fq '<string>3.4.5</string>' "$out/Zode.app/Contents/Info.plist" \
    || fail "normalized bundle version is missing"
  grep -Fq '<string>3.4.5-beta.6</string>' "$out/Zode.app/Contents/Info.plist" \
    || fail "full release version is missing"
  codesign --verify --deep --strict "$out/Zode.app" \
    || fail "ad-hoc bundle signature does not verify"
}

test_relative_output_directory_is_supported() {
  [[ "$(uname -s)" == Darwin ]] || return 0
  (
    cd "$TMP"
    ZODE_APP_BINARY=/usr/bin/true "$PACKAGER" \
      --target aarch64-apple-darwin \
      --out relative-out \
      --version 1.0.0 >/dev/null
  )
  [[ -s "$TMP/relative-out/zode-desktop-1.0.0-aarch64-apple-darwin.tar.gz" ]] \
    || fail "relative output directory did not receive the macOS archive"
}

test_packager_prefers_the_project_toolchain_when_available() {
  [[ "$(uname -s)" == Darwin ]] || return 0
  local fake_bin="$TMP/fake-bin"
  local fake_target="$TMP/fake-target"
  local fake_log="$TMP/fake-cargo.log"
  local out="$TMP/toolchain-macos"
  mkdir -p "$fake_bin"
  cat >"$fake_bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "$*" >>"$FAKE_CARGO_LOG"
if [[ "${1:-}" == "+1.94" && "${2:-}" == "--version" ]]; then
  echo "cargo 1.94.1 (test)"
  exit 0
fi
if [[ "${1:-}" != "+1.94" ]]; then
  echo "expected the project toolchain override" >&2
  exit 9
fi
shift
if [[ "${1:-}" != build ]]; then
  echo "unexpected fake cargo command: $*" >&2
  exit 10
fi
target=""
while (($#)); do
  if [[ "$1" == --target ]]; then
    target="$2"
    break
  fi
  shift
done
mkdir -p "$CARGO_TARGET_DIR/$target/release"
cp /usr/bin/true "$CARGO_TARGET_DIR/$target/release/zode-app"
EOF
  chmod +x "$fake_bin/cargo"
  PATH="$fake_bin:/usr/bin:/bin" \
    FAKE_CARGO_LOG="$fake_log" \
    CARGO_TARGET_DIR="$fake_target" \
    "$PACKAGER" --target aarch64-apple-darwin --out "$out" --version 1.0.0 >/dev/null
  grep -Fq '+1.94 build --locked --release -p zode-app --target aarch64-apple-darwin' "$fake_log" \
    || fail "packager did not build with the available project toolchain"
}

main() {
  test_required_arguments
  test_unknown_target
  test_invalid_version
  test_unknown_argument
  test_macos_dry_run_contract
  test_windows_dry_run_contract
  test_windows_version_limits
  test_linux_dry_run_contract
  test_missing_platform_tool_is_explicit
  test_packaging_assets_are_complete
  test_windows_executable_is_gui_subsystem
  test_windows_executable_embeds_brand_icon
  test_macos_native_bundle_path
  test_relative_output_directory_is_supported
  test_packager_prefers_the_project_toolchain_when_available
  echo "build-app-release contract tests passed"
}

main "$@"
