#!/usr/bin/env bash
# Checks the lines of a generated binary's `Archive` step that write the README its release
# archive holds, taken from the project $1's release build as tests/release-build/step.py reads
# it, against READMEs of each shape: each output must be exactly the line that says where the
# links work, a blank line, and the README's bytes without a leading UTF-8 byte order mark.
set -euo pipefail
project=$1
here=$(cd "$(dirname "$0")" && pwd)
fail() { echo "error: readme-note: $1" >&2; exit 1; }
script=$(uv run --quiet --no-project --with pyyaml==6.0.3 python -I "$here/step.py" \
  "$project/.github/workflows/build-release.yml" build Archive)
# shellcheck disable=SC2016 # `$stem` is the step's own text, which this matches.
lines=$(sed -n '/^note=/,/^} > "dist\/\$stem\/README.md"$/p' <<< "$script")
[ "$(head -n 1 <<< "$lines")" != "" ] && [[ $(tail -n 1 <<< "$lines") == '} > '* ]] \
  || fail "found no lines from note= to the README's write in the Archive step"
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
# The tools the step runs, as this system resolves them. macOS's are BSD's, without `--version`.
echo "bash: $(command -v bash), $(bash -c 'echo "$BASH_VERSION"')"
for tool in head tail od tr cat; do
  echo "$tool: $(command -v "$tool"), $({ "$tool" --version 2> /dev/null || echo "no --version"; } | sed -n 1p)"
done
# Runs the step's lines in $dir/case with the further variables, and fails when they fail or take
# over 20 seconds.
run() {
  local start=$SECONDS
  (cd "$dir/case" && env version=1.2.3 stem=s GITHUB_SERVER_URL=https://github.com \
    GITHUB_REPOSITORY=owner/repo "$@" bash -euo pipefail -c "$lines") || return 1
  ((SECONDS - start <= 20))
}
note="> This file's links and images are relative to https://github.com/owner/repo/tree/v1.2.3."
count=0
# Each case is a name, then the README and the bytes that must follow the note and its blank line,
# as printf formats.
while IFS='|' read -r name readme body; do
  rm -rf "$dir/case"
  mkdir -p "$dir/case/dist/s"
  if [ "$name" = "one 4 MB line" ]; then
    head -c 4000000 /dev/zero | tr '\0' x > "$dir/case/README.md"
    cp "$dir/case/README.md" "$dir/body.md"
  else
    # shellcheck disable=SC2059 # The formats come from the table below.
    printf "$readme" > "$dir/case/README.md"
    # shellcheck disable=SC2059 # The formats come from the table below.
    printf "$body" > "$dir/body.md"
  fi
  { printf '%s\n\n' "$note"; cat "$dir/body.md"; } > "$dir/expected.md"
  cp "$dir/case/README.md" "$dir/readme.md"
  run LC_ALL=C.UTF-8 || fail "the step's lines failed or took over 20 seconds on $name"
  if ! cmp -s "$dir/expected.md" "$dir/case/dist/s/README.md"; then
    echo "expected:" >&2
    od -c "$dir/expected.md" | head -n 20 >&2
    echo "written:" >&2
    od -c "$dir/case/dist/s/README.md" | head -n 20 >&2
    fail "the step wrote other than expected for $name"
  fi
  cmp -s "$dir/readme.md" "$dir/case/README.md" || fail "the step changed the README for $name"
  count=$((count + 1))
done << 'EOF'
a title and text|# T\n\nText\n|# T\n\nText\n
an empty file||
no last line break|# T\n\nText|# T\n\nText
CRLF line breaks|# T\r\n\r\nText\r\n|# T\r\n\r\nText\r\n
a byte order mark|\xef\xbb\xbf# T\n\nText\n|# T\n\nText\n
a byte order mark and CRLF|\xef\xbb\xbf# T\r\nText\r\n|# T\r\nText\r\n
a byte order mark alone|\xef\xbb\xbf|
part of a byte order mark|\xef\xbb# T\n|\xef\xbb# T\n
Latin-1 text|# T\xe9\nText \xe9\n|# T\xe9\nText \xe9\n
a NUL byte|# T\n\x00\n|# T\n\x00\n
one 4 MB line||
EOF
echo "The Archive step wrote the README with the note as expected for $count shapes."
