#!/usr/bin/env bash
# Checks the lines of a generated binary's `Archive` step that write the README its release
# archive holds, taken from the project $1's release build as tests/release-build/step.py reads
# it, against READMEs of each shape: each output must be exactly the one below, and taking out the
# line that says where the links work, with the blank line beside it, must give back the README,
# with a last line break added when it had none.
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
note="> This file's links and images are relative to https://github.com/owner/repo/tree/v1.2.3."
count=0
# Each case is a name, then the README and the output it must give as printf formats, in which %s
# stands for the note.
while IFS='|' read -r name readme expected; do
  rm -rf "$dir/case"
  mkdir -p "$dir/case/dist/s"
  # shellcheck disable=SC2059 # The formats come from the table below.
  printf "$readme" > "$dir/case/README.md"
  # shellcheck disable=SC2059 # The formats come from the table below.
  printf "$expected" "$note" > "$dir/expected.md"
  (cd "$dir/case" && version=1.2.3 stem=s GITHUB_SERVER_URL=https://github.com GITHUB_REPOSITORY=owner/repo \
    bash -euo pipefail -c "$lines") || fail "the step's lines failed on $name"
  if ! cmp --quiet "$dir/expected.md" "$dir/case/dist/s/README.md"; then
    echo "expected:" >&2
    od -c "$dir/expected.md" >&2
    echo "written:" >&2
    od -c "$dir/case/dist/s/README.md" >&2
    fail "the step wrote other than expected for $name"
  fi
  # Takes out the note's line and the blank line after it, or before it when none follows. A
  # leading byte order mark, which the bytes above place, is left out of both sides.
  bom=$'\xef\xbb\xbf'
  LC_ALL=C awk -v note="$note" -v bom="$bom" '
    NR == 1 && index($0, bom) == 1 { $0 = substr($0, length(bom) + 1) }
    { line[NR] = $0; plain = $0; sub(/\r$/, "", plain); if (plain == note) at = NR }
    END {
      blank = (at + 1 in line && line[at + 1] ~ /^\r?$/) ? at + 1 : at - 1
      for (i = 1; i <= NR; i++) if (i != at && i != blank) print line[i]
    }' "$dir/case/dist/s/README.md" > "$dir/restored.md"
  LC_ALL=C sed "1s/^$bom//" "$dir/case/README.md" > "$dir/original.md"
  if [ -s "$dir/original.md" ] && [ -n "$(tail -c 1 "$dir/original.md")" ]; then echo >> "$dir/original.md"; fi
  cmp --quiet "$dir/original.md" "$dir/restored.md" \
    || fail "taking the note out of what the step wrote for $name doesn't give back the README"
  count=$((count + 1))
done << 'EOF'
empty||%s\n\n
a title alone|# T\n|# T\n\n%s\n
a title without a last line break|# T|# T\n\n%s\n
a title, a blank line and text|# T\n\nText\n|# T\n\n%s\n\nText\n
a title and text under it|# T\nText\n|# T\n%s\n\nText\n
a title with CRLF line breaks|# T\r\n\r\nText\r\n|# T\r\n\r\n%s\r\n\r\nText\r\n
a byte order mark and a title|\xef\xbb\xbf# T\n\nText\n|\xef\xbb\xbf# T\n\n%s\n\nText\n
a byte order mark and no title|\xef\xbb\xbfText\n|\xef\xbb\xbf%s\n\nText\n
no title|Text [a](docs/a.md)\n|%s\n\nText [a](docs/a.md)\n
a badge before the title|[![CI](ci.svg)](ci)\n\n# T\n|%s\n\n[![CI](ci.svg)](ci)\n\n# T\n
text without a last line break|# T\n\nText|# T\n\n%s\n\nText
Latin-1 text under the title|# T\xe9\nText \xe9\n|# T\xe9\n%s\n\nText \xe9\n
EOF
# A 4 MB first or second line took minutes when the step trimmed a prefix or suffix from it, so
# each must take seconds, with the note on the line that says where it goes.
long() { head -c 4000000 /dev/zero | tr '\0' x; }
while IFS='|' read -r name at; do
  rm -rf "$dir/case"
  mkdir -p "$dir/case/dist/s"
  case $name in
    title) { printf '# '; long; printf '\n\nText\n'; } ;;
    *) { printf '# T\n'; long; printf '\n'; } ;;
  esac > "$dir/case/README.md"
  (cd "$dir/case" && version=1.2.3 stem=s GITHUB_SERVER_URL=https://github.com \
    GITHUB_REPOSITORY=owner/repo LC_ALL=C.UTF-8 timeout 20 bash -euo pipefail -c "$lines") \
    || fail "the step's lines failed or took over 20 seconds on a 4 MB $name"
  [ "$(sed -n "${at}p" "$dir/case/dist/s/README.md")" = "$note" ] \
    || fail "the step put the note elsewhere with a 4 MB $name"
done << 'EOF'
title|3
line under the title|2
EOF
echo "The Archive step wrote the README with the note as expected for $count shapes."
