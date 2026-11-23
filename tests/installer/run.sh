#!/usr/bin/env bash
# Usage: run.sh <project> <target>
#
# Builds and tests the installer of the generated binary <project>, whose release answer gives
# <target> an installer, in Git Bash on a Windows runner. It runs the release build's own steps
# twice, `Install Inno Setup` and `Package`, for version 0.1.0 and then 0.2.0-beta.1, with a
# release/package.sh that stages the program, its notices and tests/installer/app as window.exe,
# and writes the portable program as the target's other file. It builds 0.1.0 a second time with
# the Inno Setup 6 release pinned here, and 0.2.0-beta.1 a second time with CloseApplications
# "force" defined in release/installer-custom.iss, then runs test.ps1 on the four installers. Each
# build uses tests/installer/custom.iss as the custom file, shaped like a real program's with
# [Code] last. Before that, it compiles the installer with the custom file trellis generates, with
# one that ends in each section a project commonly adds and one whose last line has no newline,
# checks that the custom file's SetupMutex reaches [Setup], and checks that a line between the
# include and trellis's [Setup], as trellis 0.19.2 wrote, fails the [Code]-last build.
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
cp "$here/custom.iss" release/installer-custom.iss
# A 16x16 icon for the custom file's SetupIconFile: the directory, the bitmap header, the pixels
# and the mask.
uv run --quiet --no-project python -I -c '
import struct, sys
pixels = bytes([0x30, 0x80, 0xd0, 0xff]) * 256
image = struct.pack("<IiiHHIIiiII", 40, 16, 32, 1, 32, 0, len(pixels) + 64, 0, 0, 0, 0) + pixels + bytes(64)
icon = struct.pack("<HHH", 0, 1, 1) + struct.pack("<BBBBHHII", 16, 16, 0, 0, 1, 32, len(image), 22) + image
open(sys.argv[1], "wb").write(icon)
' release/demo.ico
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
# Compiles release/installer.iss for 0.1.0, with the files the last build staged, into $work/$1,
# with ISCC's output in $work/$1.log.
compile() {
  mkdir -p "$work/$1"
  MSYS_NO_PATHCONV=1 "$ISCC" /Qp "/DTarget=$target" /DAppVersion=0.1.0 \
    "/DStaging=$(cygpath -w "$RUNNER_TEMP/staging")" "/O$(cygpath -w "$work/$1")" \
    "$(cygpath -w release/installer.iss)" > "$work/$1.log" 2>&1
}
# Prints release/installer.iss preprocessed, as compile would build it. Its readers take all of it,
# without grep --quiet, so tr isn't killed by SIGPIPE, which pipefail would report as a failure.
preprocess() {
  MSYS_NO_PATHCONV=1 "$ISCC" -e "/DTarget=$target" /DAppVersion=0.1.0 \
    "/DStaging=$(cygpath -w "$RUNNER_TEMP/staging")" "$(cygpath -w release/installer.iss)" | tr -d '\r'
}
cp release/installer-custom.iss "$work/installer-custom.iss"
preprocess | grep --line-regexp 'SetupMutex=DemoBinary.Setup' > /dev/null \
  || fail "the custom file's SetupMutex doesn't reach [Setup]"
git checkout --quiet -- release/installer-custom.iss
if ! compile default; then
  cat "$work/default.log" >&2
  fail "the installer doesn't compile with the custom file trellis generates"
fi
preprocess | grep --line-regexp 'CloseApplications=yes' > /dev/null \
  || fail "the installer doesn't default CloseApplications to yes"
echo "the installer compiles with the custom file trellis generates"
printf '[Code]\nfunction Demo: Boolean;\nbegin\n  Result := True;\nend;' > release/installer-custom.iss
if ! compile no-final-newline; then
  cat "$work/no-final-newline.log" >&2
  fail "the installer doesn't compile with a custom file whose last line has no newline"
fi
echo "the installer compiles with a custom file whose last line has no newline"
while IFS='|' read -r section entry; do
  printf '; Ends in [%s].\n\n[%s]\n%b\n' "$section" "$section" "$entry" > release/installer-custom.iss
  if ! compile "section-$section"; then
    cat "$work/section-$section.log" >&2
    fail "the installer doesn't compile with a custom file that ends in [$section]"
  fi
  echo "the installer compiles with a custom file that ends in [$section]"
done << 'SECTIONS'
Code|function Demo: Boolean;\nbegin\n  Result := True;\nend;
Run|Filename: "{app}\\demo-binary.exe"; Parameters: "--version"; Flags: nowait postinstall skipifsilent
UninstallDelete|Type: filesandordirs; Name: "{localappdata}\\DemoBinary\\updates"
InstallDelete|Type: files; Name: "{app}\\LICENSE.txt"
Setup|SetupIconFile=demo.ico
Files|Source: "{#Staging}\\THIRD-PARTY-LICENSES.html"; DestDir: "{app}\\docs"; Flags: ignoreversion
Icons|Name: "{autoprograms}\\Demo Binary notices"; Filename: "{app}\\THIRD-PARTY-LICENSES.html"
Tasks|Name: "extra"; Description: "An extra task"; Flags: unchecked
SECTIONS
# A comment line between the include and [Setup], as trellis 0.19.2 wrote, lands in the custom
# file's [Code], so the [Code]-last build must fail with it.
cp release/installer.iss "$work/installer.iss"
include=$(grep --line-number --max-count 1 '^#include "installer-custom.iss"$' release/installer.iss | cut -d: -f1)
sed -i 's/^#include "installer-custom.iss"$/&\n; A comment after the include./' release/installer.iss
grep --quiet '^; A comment after the include\.$' release/installer.iss || fail "found no include to put a comment after"
cp "$work/installer-custom.iss" release/installer-custom.iss
if compile comment-after-include; then
  fail "the installer compiled with a comment between the include and [Setup] and [Code] last"
fi
# Inno Setup reports an error in an included file at the include's line, so another failure,
# such as a missing file in [Setup], doesn't pass for this one.
grep --after-context 1 'Error on line' "$work/comment-after-include.log" || true
grep --quiet --extended-regexp "Error on line ($include|$((include + 1))) in .*installer\.iss" \
  "$work/comment-after-include.log" || fail "the [Code]-last build failed, but not at the include"
echo "a comment between the include and [Setup] fails the [Code]-last build, as it should"
cp "$work/installer.iss" release/installer.iss
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
git checkout --quiet -- Cargo.toml Cargo.lock release/installer-custom.iss
rm -rf release/package.sh release/demo.ico target/release-files
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
