#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_file() {
  [ -f "$1" ] || fail "expected file to exist: $1"
}

assert_contains() {
  file="$1"
  needle="$2"
  if ! grep -F "$needle" "$file" >/dev/null 2>&1; then
    printf '--- %s ---\n' "$file" >&2
    sed -n '1,160p' "$file" >&2
    fail "expected output to contain: $needle"
  fi
}

make_asset() {
  dir="$1"
  asset="$dir/zode-0.1.0-beta.1-arm64-mac.tar.gz"
  mkdir -p "$dir/payload"
  cat >"$dir/payload/zode" <<'EOF'
#!/bin/sh
printf 'zode 0.1.0-beta.1\n'
EOF
  chmod 755 "$dir/payload/zode"
  tar -czf "$asset" -C "$dir/payload" zode
}

test_install_sh_falls_back_to_release_atom_when_api_is_forbidden() {
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT INT TERM

  make_asset "$tmp"
  mkdir -p "$tmp/bin" "$tmp/fakebin"

  cat >"$tmp/fakebin/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) printf 'Darwin\n' ;;
  -m) printf 'arm64\n' ;;
  *) exit 1 ;;
esac
EOF
  chmod 755 "$tmp/fakebin/uname"

  cat >"$tmp/fakebin/curl" <<EOF
#!/bin/sh
out=''
url=''
while [ "\$#" -gt 0 ]; do
  case "\$1" in
    -o) out="\$2"; shift 2 ;;
    -*) shift ;;
    *) url="\$1"; shift ;;
  esac
done

case "\$url" in
  https://api.github.com/repos/ZSeven-W/zode/releases)
    printf 'curl: (56) The requested URL returned error: 403\n' >&2
    exit 22
    ;;
  https://github.com/ZSeven-W/zode/releases.atom)
    cat <<'ATOM'
<?xml version="1.0" encoding="UTF-8"?>
<feed>
  <entry>
    <title>v0.1.0-beta.1</title>
    <link rel="alternate" type="text/html" href="https://github.com/ZSeven-W/zode/releases/tag/v0.1.0-beta.1"/>
  </entry>
</feed>
ATOM
    ;;
  https://github.com/ZSeven-W/zode/releases/download/v0.1.0-beta.1/zode-0.1.0-beta.1-arm64-mac.tar.gz)
    cp "$tmp/zode-0.1.0-beta.1-arm64-mac.tar.gz" "\$out"
    ;;
  *)
    printf 'unexpected curl url: %s\n' "\$url" >&2
    exit 1
    ;;
esac
EOF
  chmod 755 "$tmp/fakebin/curl"

  if ! PATH="$tmp/fakebin:$PATH" ZODE_BIN_DIR="$tmp/bin" sh "$ROOT/scripts/install.sh" >"$tmp/out" 2>"$tmp/err"; then
    sed -n '1,160p' "$tmp/out" >&2
    sed -n '1,160p' "$tmp/err" >&2
    fail "installer failed"
  fi

  assert_file "$tmp/bin/zode"
  assert_contains "$tmp/out" "installing zode v0.1.0-beta.1 (arm64-mac)"
  "$tmp/bin/zode" --version >"$tmp/version"
  assert_contains "$tmp/version" "zode 0.1.0-beta.1"
}

test_install_sh_falls_back_to_release_atom_when_api_is_forbidden
printf 'ok - install.sh falls back to releases.atom when GitHub API is forbidden\n'
