#!/usr/bin/env bash
# Usage: run.sh <project> <target>
#
# Builds and tests the installer of the generated binary <project>, whose release answer gives
# <target> an installer, in Git Bash on a Windows runner. It runs the release build's own steps
# twice, `Install Inno Setup` and `Package`, for version 0.1.0 and then 0.2.0-beta.1, with a
# release/package.sh that stages the program, its notices and tests/installer/app as window.exe,
# and writes the portable program as the target's other file. It builds 0.1.0 a second time with
# the Inno Setup 6 release pinned here, and 0.2.0-beta.1 a second time with CloseApplications
# "force" defined in release/installer-custom.iss, then runs test.ps1 on the four installers.
set -euo pipefail
project=$1 target=$2
here=$(cd "$(dirname "$0")" && pwd)
fail() { echo "error: installer test: $1" >&2; exit 1; }
[ -n "${RUNNER_TEMP:-}" ] || fail "this runs on a GitHub Actions Windows runner"
command -v cygpath > /dev/null || fail "this runs in Git Bash on Windows"
inno6_url=https://github.com/jrsoftware/issrc/releases/download/is-6_7_4/innosetup-6.7.4.exe
inno6_sha256=a549929319f9120716677c685535ca4671e4b937b5fdc87e2585f32d8eb30e8c
work="$RUNNER_TEMP/installer-test"
rm -rf "$work"
mkdir -p "$work"
cd "$project"
workflow=.github/workflows/build-release.yml
# Prints the build job's matrix value $2 for the target $1.
matrix() {
  uv run --quiet --no-project --with pyyaml==6.0.3 python -I -c '
import sys, yaml
entries = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))["jobs"]["build"]["strategy"]["matrix"]["include"]
print(next(entry for entry in entries if entry["target"] == sys.argv[2]).get(sys.argv[3], ""))
' "$workflow" "$1" "$2"
}
# Runs the build job's step named $1 as the runner would.
step() {
  local script
  script=$(uv run --quiet --no-project --with pyyaml==6.0.3 python -I "$here/../release-build/step.py" "$workflow" build "$1")
  RUNNER_OS=Windows GITHUB_OUTPUT=/dev/null GITHUB_ENV="$work/github-env" bash -eo pipefail -c "$script"
}
files=$(matrix "$target" files)
installer=$(matrix "$target" installer)
[ -n "$installer" ] || fail "the build job's matrix gives $target no installer"
mkdir release 2> /dev/null || true
cat > release/package.sh << 'EOF'
#!/usr/bin/env bash
# Stages the program, its notices and the test's helper to install, and writes the portable
# program as the target's other release file.
set -euo pipefail
cp "$BINARY" "$STAGING/$NAME.exe"
cp "$NOTICES" "$STAGING/THIRD-PARTY-LICENSES.html"
cp "$HELPER" "$STAGING/window.exe"
cp "$BINARY" "$DIST/$NAME-$VERSION-portable.exe"
EOF
for build in 1 2; do
  HELPER_BUILD=$build cargo build --quiet --release --manifest-path "$here/app/Cargo.toml" \
    --target "$target" --target-dir "$work/helper"
  cp "$work/helper/$target/release/installer-helper.exe" "$work/helper-$build.exe"
done
just notices > /dev/null
: > "$work/github-env"
step "Install Inno Setup"
ISCC=$(sed -n 's/^ISCC=//p' "$work/github-env")
[ -n "$ISCC" ] || fail "Install Inno Setup set no ISCC"
export ISCC TARGET=$target FILES=$files INSTALLER=$installer
# Builds the release files of the version the workspace has into $work/$1 with the helper $2.
package() {
  cargo build --quiet --release --target "$target"
  HELPER="$work/helper-$2.exe" step Package
  mkdir "$work/$1"
  mv dist/* "$work/$1/"
  ls "$work/$1"
}
package v1 1
# Inno Setup 6 builds the same version from the same script, for the upgrade from a project's
# installers before trellis pinned Inno Setup 7.
curl --fail --silent --show-error --location --retry 3 --output "$work/innosetup6.exe" "$inno6_url"
echo "$inno6_sha256  $work/innosetup6.exe" | sha256sum --check --strict --quiet
MSYS_NO_PATHCONV=1 "$work/innosetup6.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /CURRENTUSER \
  /NOICONS /PORTABLE=1 "/DIR=$(cygpath -w "$work/inno6")" "/LOG=$(cygpath -w "$work/inno6.log")"
ISCC="$work/inno6/ISCC.exe" package v1-inno6 1
sed -i 's/^version = "0\.1\.0"$/version = "0.2.0-beta.1"/' Cargo.toml
grep --quiet '^version = "0.2.0-beta.1"$' Cargo.toml || fail "found no workspace version to bump"
package v2 2
# The same build with the custom file's define that kills a windowless process.
cp release/installer-custom.iss "$work/installer-custom.iss"
printf '\n#define CloseApplications "force"\n' >> release/installer-custom.iss
package v2-force 2
cp "$work/installer-custom.iss" release/installer-custom.iss
git checkout --quiet -- Cargo.toml Cargo.lock
rm -rf release/package.sh target/release-files
app_id=$(sed -n 's/^ *#define AppId "\(.*\)"$/\1/p' release/installer.iss | head -n 1)
app_name=$(sed -n 's/^ *#define AppName "\(.*\)"$/\1/p' release/installer.iss | head -n 1)
name=$(basename "$(pwd)")
version_of() { echo "${installer//\{version\}/$1}"; }
pwsh -NoProfile -NonInteractive -File "$(cygpath -w "$here/test.ps1")" \
  -V1 "$(cygpath -w "$work/v1/$(version_of 0.1.0)")" \
  -V1Inno6 "$(cygpath -w "$work/v1-inno6/$(version_of 0.1.0)")" \
  -V2 "$(cygpath -w "$work/v2/$(version_of 0.2.0-beta.1)")" \
  -V2Force "$(cygpath -w "$work/v2-force/$(version_of 0.2.0-beta.1)")" \
  -Helper1 "$(cygpath -w "$work/helper-1.exe")" -Helper2 "$(cygpath -w "$work/helper-2.exe")" \
  -AppId "$app_id" -AppName "$app_name" -Exe "$name.exe" -Logs "$(cygpath -w "$work/logs")"
