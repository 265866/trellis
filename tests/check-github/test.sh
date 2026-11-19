#!/usr/bin/env bash
# Runs `just check-github` in the generated project $1 against the fixtures next to this script,
# with tests/check-github/gh as gh: a correct public repository must pass and one with merge
# commits allowed must fail on that setting; a private repository without a ruleset must pass with
# main's protection unavailable.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
cd "$1"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
mkdir "$out/bin"
cp "$here/gh" "$out/bin/gh"
fail() { echo "error: check-github: $1" >&2; exit 1; }

# Copies the public fixtures to $out/$1 and applies the jq program $3 to the fixture named $2.
variant() {
  cp -R "$here/public" "$out/$1"
  jq "$3" "$here/public/$2.json" > "$out/$1/$2.json"
}

# Runs check-github on the fixtures in $out/$1, expecting exit status $2 and a line $3.
expect() {
  local status=0 report
  report=$(GH_FIXTURES="$out/$1" PATH="$out/bin:$PATH" just check-github 2>&1) || status=$?
  if [ "$status" != "$2" ] || ! grep --quiet --line-regexp --fixed-strings -- "$3" <<< "$report"; then
    echo "$report" >&2
    fail "$1: expected exit status $2 and the line: $3"
  fi
}

if grep --quiet '^visibility: public$' .copier-answers.yml; then
  variant correct repository .
  expect correct 0 "ok: main's required checks and rules"
  variant merge-commits repository '.allow_merge_commit = true'
  expect merge-commits 1 'wrong: merge settings: "squash true, merge commits true, rebase false, squash commit PR_TITLE and PR_BODY", expected "squash true, merge commits false, rebase false, squash commit PR_TITLE and PR_BODY"'
else
  variant no-ruleset repository '.visibility = "private"'
  echo '[]' > "$out/no-ruleset/rulesets.json"
  echo '[]' > "$out/no-ruleset/rules-branches-main.json"
  expect no-ruleset 0 "unavailable: main's protection: there's no ruleset, and rulesets in a private repository need GitHub Pro or a paid organization plan; on one, run just setup-github"
fi
echo "check-github reports the fixtures as expected."
