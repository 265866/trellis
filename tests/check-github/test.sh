#!/usr/bin/env bash
# Runs `just check-github` in the generated project $1 against the fixtures next to this script,
# with tests/check-github/gh as gh. A correct repository must pass. One with merge commits allowed,
# one whose ruleset main is disabled or in evaluate mode, lets an actor bypass it, leaves rules
# to another ruleset or applies no rule to main, and one without it must each fail and say why. A
# private repository whose plan GitHub says lacks rulesets must pass with main's protection
# unavailable, though gh writes the refusal's body to stdout. A description other than the
# answer's, without its backticks, is wrong, and so are topics other than a topics answer's, in any
# order. Without a topics answer, topics on GitHub are a note that doesn't fail.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
cd "$1"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
mkdir "$out/bin"
cp "$here/gh" "$out/bin/gh"
fail() { echo "error: check-github: $1" >&2; exit 1; }

# The fixtures in public/ describe a correct public repository. A private one differs in its
# visibility, and the checks skip what only a public repository has. The description and topics
# come from the project's answers, the topics in reverse order.
answers=$(uv run --quiet --no-project --with pyyaml==6.0.3 python -I -c \
  'import json, yaml; print(json.dumps(yaml.safe_load(open(".copier-answers.yml", encoding="utf-8"))))')
description=$(jq --raw-output '.description | gsub("`"; "")' <<< "$answers")
topics=$(jq --raw-output '.topics // [] | sort | join(", ")' <<< "$answers")
cp -R "$here/public" "$out/correct"
jq --argjson answers "$answers" '.visibility = $answers.visibility
  | .description = ($answers.description | gsub("`"; "")) | .topics = ($answers.topics // [] | reverse)' \
  "$here/public/repository.json" > "$out/correct/repository.json"

# Copies the correct fixtures to $out/$1 and applies the jq program $3 to the fixture named $2.
variant() {
  cp -R "$out/correct" "$out/$1"
  jq "$3" "$out/correct/$2.json" > "$out/$1/$2.json"
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

expect correct 0 "ok: main's required checks and rules"
expect correct 0 "ok: description"
variant other-description repository '.description = "Another project."'
expect other-description 1 "wrong: description: \"Another project.\", expected \"$description\""
variant no-description repository '.description = null'
expect no-description 1 "wrong: description: \"nothing\", expected \"$description\""
if [ -n "$topics" ]; then
  expect correct 0 "ok: topics"
  variant other-topics repository '.topics = ["rust"]'
  expect other-topics 1 "wrong: topics: \"rust\", expected \"$topics\""
  variant no-topics repository '.topics = []'
  expect no-topics 1 "wrong: topics: \"nothing\", expected \"$topics\""
else
  expect correct 0 "ok: topics: none, and no topics answer"
  variant own-topics repository '.topics = ["sms", "rust", "2024"]'
  expect own-topics 0 "note: topics: GitHub has 2024, rust, sms, and with no topics answer setup-github leaves them; to keep them in the answers, run uvx copier update --defaults --vcs-ref=:current: --data 'topics=[\"2024\",\"rust\",\"sms\"]'"
fi
variant merge-commits repository '.allow_merge_commit = true'
expect merge-commits 1 'wrong: merge settings: "squash true, merge commits true, rebase false, squash commit PR_TITLE and PR_BODY", expected "squash true, merge commits false, rebase false, squash commit PR_TITLE and PR_BODY"'
# GitHub's rules for a branch leave out a ruleset that isn't active.
variant disabled rulesets '.[0].enforcement = "disabled"'
echo '[]' > "$out/disabled/rules-branches-main.json"
expect disabled 1 "wrong: main's protection: the ruleset main is disabled, so it protects nothing; run just setup-github"
variant evaluate rulesets '.[0].enforcement = "evaluate"'
echo '[]' > "$out/evaluate/rules-branches-main.json"
expect evaluate 1 "wrong: main's protection: the ruleset main is in evaluate mode, so it protects nothing; run just setup-github"
variant bypass rulesets-1 '.bypass_actors = [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}]'
expect bypass 1 'wrong: what may bypass main'"'"'s ruleset: "RepositoryRole 5 (always)", expected "nothing"'
# GitHub leaves the list out for an account that may not change the ruleset.
variant bypass-hidden rulesets-1 'del(.bypass_actors)'
expect bypass-hidden 1 'wrong: what may bypass main'"'"'s ruleset: "unreadable", expected "nothing"'
# A second ruleset, which anything might bypass, holds the pull request rule and the checks.
variant split rules-branches-main 'map(if .type == "pull_request" or .type == "required_status_checks" then .ruleset_id = 2 end)'
expect split 1 'wrong: main'"'"'s required checks and rules: "; deletion, non_fast_forward", expected "CI passed from GitHub Actions, PR title from GitHub Actions, Rule changes stand alone from GitHub Actions; deletion, non_fast_forward, pull_request"'
variant another-branch rules-branches-main '[]'
expect another-branch 1 "wrong: main's protection: the ruleset main is active but applies no rule to the branch main; run just setup-github"
variant deleted rulesets '[]'
echo '[]' > "$out/deleted/rules-branches-main.json"
expect deleted 1 "wrong: main's protection: there's no ruleset named main; run just setup-github"

if grep --quiet '^visibility: private$' .copier-answers.yml; then
  upgrade='gh: Upgrade to GitHub Pro or make this repository public to enable this feature. (HTTP 403)'
  unavailable="unavailable: main's protection: GitHub says rulesets in a private repository need GitHub Pro or a paid organization plan; on one, run just setup-github"
  cp -R "$out/deleted" "$out/plan-lacks-rulesets"
  echo "$upgrade" > "$out/plan-lacks-rulesets/rulesets.error"
  echo "$upgrade" > "$out/plan-lacks-rulesets/branches-main-protection-required_status_checks.error"
  expect plan-lacks-rulesets 0 "$unavailable"
  # GitHub documents no answer from the rulesets of such a plan's repository. If it lists none,
  # classic branch protection's answer still tells the plan from a deleted ruleset.
  cp -R "$out/deleted" "$out/plan-lists-no-rulesets"
  echo "$upgrade" > "$out/plan-lists-no-rulesets/branches-main-protection-required_status_checks.error"
  expect plan-lists-no-rulesets 0 "$unavailable"
fi
echo "check-github reports the fixtures as expected."
