#!/usr/bin/env bash
# Usage: setup-values.sh <script.iss> ["/D<name>=<value>"...]
#
# Prints what an Inno Setup script sets, its `#define`s and `#include`s resolved by Inno Setup's
# own preprocessor, as setup-values.py describes, sorted, so a diff of two scripts shows only the
# entries that differ, whatever order their sections come in. Pass the defines the script requires,
# each quoted, since Git Bash drops the backslashes of an unquoted Windows path, such as
# `"/DTarget=x86_64-pc-windows-msvc" "/DAppVersion=1.0.0" "/DStaging=C:\staging"` for a trellis
# release/installer.iss. It runs in Git Bash on Windows, with ISCC naming Inno Setup 7.1 or later's
# ISCC.exe. To compare a project's old script with the one trellis writes:
#
#   diff <(setup-values.sh old.iss "/DAppVersion=1.0.0" ...) \
#     <(setup-values.sh release/installer.iss "/DTarget=x86_64-pc-windows-msvc" "/DAppVersion=1.0.0" ...)
#
# `ISCC -e` only preprocesses: it doesn't compile the script, so it doesn't check that [Code]
# parses or that the files and icon it names exist. The comparison can't stand in for a build;
# compile the installer, as the release build does, to know it builds.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
script=$1
shift
out=$(mktemp)
trap 'rm -f "$out"' EXIT
# Git Bash would rewrite ISCC's `/` options as paths.
MSYS_NO_PATHCONV=1 "${ISCC:?set ISCC to the ISCC.exe of Inno Setup 7.1 or later}" -e "$@" "$(cygpath -w "$script")" > "$out"
uv run --quiet --no-project python -I "$here/setup-values.py" "$out" | LC_ALL=C sort
