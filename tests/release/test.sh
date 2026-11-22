#!/usr/bin/env bash
# Runs `just changelog` and `just release` from the justfile $1 in a scratch repository tagged
# v0.1.0 and v0.4.0, with a fake gh that logs each call. `changelog` must accept `Unreleased`, the
# newest tag and each next version the bump rule allows, and reject the rest; `release` must tag
# and publish a new version once, publish only the release a failed run missed, and do nothing for
# a version another commit has, that another run tagged first, or that a newer tag holds.
set -euo pipefail
# Keeps the user's git config, such as commit signing or hooks, out of the scratch repository.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
justfile=$1
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
fail() { echo "error: release test: $1" >&2; exit 1; }
mkdir "$out/bin" "$out/repo"
cat > "$out/bin/gh" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$FAKE_LOG"
case "$1 $2" in
  "api repos/{owner}/{repo}/git/tags") echo fake-tag-sha ;;
  "api repos/{owner}/{repo}/git/refs") [ "${FAKE_REF:-new}" = new ] ;;
  "api repos/{owner}/{repo}/git/ref/tags/"*) [ "${FAKE_REF:-new}" = raced ] ;;
  "release view") [ "$FAKE_RELEASE" = exists ] ;;
  "release create")
    while [ "$1" != --notes ]; do shift; done
    printf '%s\n' "$2" > "$FAKE_NOTES" ;;
  *) echo "error: the fake gh doesn't serve $*" >&2; exit 2 ;;
esac
EOF
chmod +x "$out/bin/gh"
cd "$out/repo"
git init --quiet --initial-branch main
cp "$justfile" justfile
commit() { git -c user.name=trellis -c user.email=trellis@localhost "$@"; }
intro='# Changelog

Intro text.
'
older='## v0.4.0

### Action required in projects

- An older ask.

## v0.3.0 and earlier

No notes.'
# Writes CHANGELOG.md with the newest heading $1 and its body $2 above the older sections, and
# commits it.
changelog() {
  printf '%s\n## %s\n\n%s\n\n%s\n' "$intro" "$1" "$2" "$older" > CHANGELOG.md
  commit add --all
  commit commit --quiet --allow-empty --message "$1"
}
changelog v0.1.0 ''
commit tag --annotate --message v0.1.0 v0.1.0
changelog v0.4.0 ''
commit tag --annotate --message v0.4.0 v0.4.0

asks='### Action required in projects

- Run a step.

### Changed

- A change.'
changed='### Action required in projects

### Changed

- A change.'

# Writes the newest section as `changelog` does, then runs `just changelog`, expecting exit status
# $3 and output $4, or with a failure, a line of stderr containing $4.
expect() {
  changelog "$1" "$2"
  local status=0 output
  output=$(just changelog 2> "$out/stderr") || status=$?
  if [ "$status" != "$3" ]; then
    cat "$out/stderr" >&2
    fail "just changelog for \"## $1\" exited $status, not $3"
  fi
  if [ "$3" = 0 ]; then
    [ "$output" = "$4" ] || fail "just changelog for \"## $1\" printed \"$output\", not \"$4\""
  elif ! grep --quiet --fixed-strings -- "$4" "$out/stderr"; then
    cat "$out/stderr" >&2
    fail "just changelog for \"## $1\" didn't say: $4"
  fi
}

expect Unreleased "$asks" 0 ''
expect v0.4.0 '' 0 v0.4.0
expect v0.5.0 "$asks" 0 v0.5.0
expect v0.4.1 "$changed" 0 v0.4.1
expect v0.4.1 '- A fix.' 0 v0.4.1
expect v0.4.1 '### Action required in projects

* Run a step.' 1 'so it must be v0.5.0'
expect v0.4.1 '### Action required in projects

1. Run a step.' 1 'so it must be v0.5.0'
expect v1.0.0 "$asks" 0 v1.0.0
expect v1.0.0 "$changed" 0 v1.0.0
expect v0.5.0 "$changed" 1 'its "Action required in projects" lists nothing, so it must be v0.4.1'
expect v0.4.1 "$asks" 1 'asks something of projects, so it must be v0.5.0'
expect v0.6.0 "$asks" 1 'v0.6.0 must follow v0.4.0 as v0.4.1, v0.5.0 or v1.0.0'
expect v0.1.0 '' 1 'names v0.1.0, which is older than the newest tag, v0.4.0'
expect 0.5.0 "$asks" 1 'must be "## Unreleased" or a version'
expect v0.5 "$asks" 1 'must be "## Unreleased" or a version'
expect v0.05.0 "$asks" 1 'must be "## Unreleased" or a version'
expect 'v0.5.0 and earlier' "$asks" 1 'must be "## Unreleased" or a version'
commit tag --annotate --message v1.0.0 v1.0.0
expect v1.0.1 "$asks" 0 v1.0.1
expect v1.1.0 "$changed" 0 v1.1.0
commit tag --delete v0.1.0 v0.4.0 v1.0.0 > /dev/null
expect v0.4.0 '' 1 'found no vX.Y.Z tag'
commit tag --annotate --message v0.1.0 v0.1.0 "$(git rev-list --max-parents=0 HEAD)"
commit tag --annotate --message v0.4.0 v0.4.0 HEAD~1

# Runs `just release` with the fake release $1 (`exists` or `missing`), expecting gh to have been
# called exactly as $2 lists, one call a line.
release() {
  rm -f "$out/log" "$out/notes"
  touch "$out/log"
  FAKE_LOG="$out/log" FAKE_NOTES="$out/notes" FAKE_RELEASE=$1 PATH="$out/bin:$PATH" just release > /dev/null \
    || fail "just release failed"
  if [ "$(cat "$out/log")" != "$2" ]; then
    printf 'gh was called with:\n%s\n' "$(cat "$out/log")" >&2
    fail "expected gh to be called with:
$2"
  fi
}

changelog Unreleased "$asks"
release missing ''

changelog v0.4.0 '- A note on a later commit.'
release missing ''
# A commit before the one that adds the next section, which that version's tag will hold.
held=$(git rev-parse HEAD)

changelog v0.5.0 "$asks

- A draft note."
# A commit after the one that added the section, whose CI may pass first, must not get the tag.
commit commit --quiet --allow-empty --message 'A commit that leaves the section alone'
release missing ''

changelog v0.5.0 "$asks"
head=$(git rev-parse HEAD)
tag_calls="api repos/{owner}/{repo}/git/tags -f tag=v0.5.0 -f message=v0.5.0 -f object=$head -f type=commit --jq .sha
api repos/{owner}/{repo}/git/refs -f ref=refs/tags/v0.5.0 -f sha=fake-tag-sha"
# Another run created the tag between this run's checkout and its own attempt.
FAKE_REF=raced release missing "$tag_calls
api repos/{owner}/{repo}/git/ref/tags/v0.5.0"
# GitHub refused the tag for another reason, which must fail the job.
if FAKE_LOG="$out/log" FAKE_NOTES="$out/notes" FAKE_RELEASE=missing FAKE_REF=refused PATH="$out/bin:$PATH" \
  just release > /dev/null 2>&1; then
  fail "just release passed when GitHub refused the tag"
fi
release missing "api repos/{owner}/{repo}/git/tags -f tag=v0.5.0 -f message=v0.5.0 -f object=$head -f type=commit --jq .sha
api repos/{owner}/{repo}/git/refs -f ref=refs/tags/v0.5.0 -f sha=fake-tag-sha
release view v0.5.0
release create v0.5.0 --verify-tag --title v0.5.0 --notes $asks"
[ "$(cat "$out/notes")" = "$asks" ] || fail "the release notes were \"$(cat "$out/notes")\", not the v0.5.0 section"

# The fake gh created no tag, so these make it, as GitHub would have.
commit tag --annotate --message v0.5.0 v0.5.0
release missing "release view v0.5.0
release create v0.5.0 --verify-tag --title v0.5.0 --notes $asks"
release exists 'release view v0.5.0'

commit commit --quiet --allow-empty --message 'A later commit'
release missing ''

# The earlier commit's run finishes after v0.5.0 is tagged on a later commit: its CHANGELOG.md
# still names v0.4.0, older than the newest tag, and it must do nothing rather than fail.
git checkout --quiet "$held"
release missing ''
git checkout --quiet main

# A commit adds v0.6.0, and a later one adds v0.6.1, whose run tags first. The earlier commit's
# version has no tag, so its run must fail rather than leave v0.6.0 unreleased without a word.
changelog v0.6.0 "$asks"
overtaken=$(git rev-parse HEAD)
changelog v0.6.1 '- A fix.'
commit tag --annotate --message v0.6.1 v0.6.1
git checkout --quiet "$overtaken"
if FAKE_LOG="$out/log" FAKE_NOTES="$out/notes" FAKE_RELEASE=missing PATH="$out/bin:$PATH" \
  just release > /dev/null 2>&1; then
  fail "just release passed on a commit whose version a later tag overtook"
fi
git checkout --quiet main
echo "just changelog and just release behave as expected."
