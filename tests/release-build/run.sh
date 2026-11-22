#!/usr/bin/env bash
# Runs the release build's steps for the host's target in the generated binary project $1, as the
# release workflow's build job would on a Linux runner, and lists the files it leaves in dist/.
# It deletes what an earlier build left, builds the workspace for the host's own triple, writes
# the notices, then runs the job's own steps that build the web UI and write the release files,
# `Build the web UI` when the project has a web UI, and `Archive`, or the `Package` step that
# replaces it, with the caller's FILES. It runs only on Linux, whose runner the steps assume.
set -euo pipefail
project=$1
here=$(cd "$(dirname "$0")" && pwd)
fail() { echo "error: release build: $1" >&2; exit 1; }
[ "$(uname -s)" = Linux ] || fail "this runs only on Linux"
cd "$project"
target=$(rustc -vV | sed -n 's/^host: //p')
# Prints the script of the build job's step named $1; exits with 2 when the job has no such step.
step() {
  uv run --quiet --no-project --with pyyaml==6.0.3 python -I "$here/step.py" \
    .github/workflows/build-release.yml build "$1"
}
rm -rf dist crates/*/web/dist target/THIRD-PARTY-LICENSES*
just notices > /dev/null
cargo build --quiet --locked --release --target "$target"
web=no
if compgen -G 'crates/*/web/package.json' > /dev/null; then web=yes; fi
packaged=no
for name in "Build the web UI" Archive Package; do
  status=0
  script=$(step "$name") || status=$?
  if [ "$status" -eq 2 ]; then
    if [ "$name" = "Build the web UI" ] && [ "$web" = yes ]; then
      fail "the project has a web UI, but the release build has no step named $name"
    fi
    continue
  fi
  [ "$status" -eq 0 ] || exit "$status"
  if [ "$name" != "Build the web UI" ]; then packaged=yes; fi
  TARGET=$target RUNNER_OS=Linux GITHUB_OUTPUT=/dev/null bash -euo pipefail -c "$script" >&2
done
[ "$packaged" = yes ] || fail "the release build has no Archive or Package step"
find dist -type f | sort
