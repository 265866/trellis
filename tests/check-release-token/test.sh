#!/usr/bin/env bash
# Runs `just check-release-token` and `just set-release-token` in the generated project $1 with a
# fake gh that answers each request with the status the test sets, as GitHub does for a token with
# or without the access: a good token passes, an expired one, one without access to the repository,
# one whose account can't push and one that can't write pull requests each fail and say why, one
# in its last 14 days passes with a warning, and set-release-token stores only a token that passes.
set -euo pipefail
cd "$1"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
mkdir "$out/bin"
cat > "$out/bin/gh" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "$1 $2" = "secret set" ]; then
  cat > "$FAKE_SECRET"
  exit 0
fi
[ "$GH_TOKEN" = good-token ] || { printf 'HTTP/2.0 401 Unauthorized\r\n\r\n{}'; exit 1; }
case "$5" in
  repos/*/git/refs) status=$FAKE_REFS ;;
  repos/*/pulls) status=$FAKE_PULLS ;;
  repos/*) status=$FAKE_REPO ;;
  *) echo "error: the fake gh doesn't serve $*" >&2; exit 2 ;;
esac
printf 'HTTP/2.0 %s Status\r\n' "$status"
if [ -n "${FAKE_EXPIRES:-}" ]; then
  printf 'Github-Authentication-Token-Expiration: %s\r\n' "$FAKE_EXPIRES"
fi
printf '\r\n{"permissions":{"push":%s}}' "${FAKE_PUSH:-true}"
[ "${status:0:1}" = 2 ]
EOF
chmod +x "$out/bin/gh"
fail() { echo "error: check-release-token: $1" >&2; exit 1; }
# Each run sets GITHUB_ACTIONS only as GITHUB_ACTIONS_VALUE says, whether or not this runs in CI.
unset GITHUB_ACTIONS
GITHUB_ACTIONS_VALUE=
far=$(date -u -d '+90 days' '+%F %T UTC' 2> /dev/null || date -u -v+90d '+%F %T UTC')
near=$(date -u -d '+3 days' '+%F %T UTC' 2> /dev/null || date -u -v+3d '+%F %T UTC')

# Runs check-release-token with the token $1 and the statuses for the repository, refs and pulls
# in $2, $3 and $4, expiring at $5, expecting exit status $6 and a line $7.
expect() {
  local status=0 report
  report=$(env ${GITHUB_ACTIONS_VALUE:+"GITHUB_ACTIONS=$GITHUB_ACTIONS_VALUE"} "RELEASE_PLZ_TOKEN=$1" "FAKE_REPO=$2" \
    "FAKE_REFS=$3" "FAKE_PULLS=$4" "FAKE_EXPIRES=$5" PATH="$out/bin:$PATH" just check-release-token 2>&1) || status=$?
  if [ "$status" != "$6" ] || ! grep --quiet --line-regexp --fixed-strings -- "$7" <<< "$report"; then
    echo "$report" >&2
    fail "expected exit status $6 and the line: $7"
  fi
}

expect good-token 200 422 422 "$far" 0 "ok: the token can write pull requests"
expect good-token 200 422 422 '' 0 "ok: the token can write contents"
expect bad-token 200 422 422 "$far" 1 "error: GitHub rejects the token: it has expired, been revoked or been mistyped. docs/maintaining.md says which access the token needs, under Before the first push, and how to replace it, under Renew the release token."
expect good-token 404 422 422 "$far" 1 "error: The token has no access to 265866/demo-shape. docs/maintaining.md says which access the token needs, under Before the first push, and how to replace it, under Renew the release token."
expect good-token 200 422 403 "$far" 1 "error: The token can't write pull requests in 265866/demo-shape. docs/maintaining.md says which access the token needs, under Before the first push, and how to replace it, under Renew the release token."
expect good-token 200 422 422 "$near" 0 "warning: The release token expires $near. Replace it before then; docs/maintaining.md says which access the token needs, under Before the first push, and how to replace it, under Renew the release token."
GITHUB_ACTIONS_VALUE=true expect good-token 200 422 403 "$far" 1 "::error::The token can't write pull requests in 265866/demo-shape. docs/maintaining.md says which access the token needs, under Before the first push, and how to replace it, under Renew the release token."
FAKE_PUSH=false expect good-token 200 422 422 "$far" 1 "error: The token's account can't push to 265866/demo-shape. Create the token with an account that has write access. docs/maintaining.md says which access the token needs, under Before the first push, and how to replace it, under Renew the release token."
expect '' 200 422 422 "$far" 1 "error: RELEASE_PLZ_TOKEN is empty or missing. docs/maintaining.md says which access the token needs, under Before the first push, and how to replace it, under Renew the release token."

secret="$out/secret"
for token in good-token bad-token; do
  rm -f "$secret"
  status=0
  printf '%s\n' "$token" | env -u GITHUB_ACTIONS FAKE_SECRET="$secret" FAKE_REPO=200 FAKE_REFS=422 FAKE_PULLS=422 PATH="$out/bin:$PATH" \
    just set-release-token > /dev/null 2>&1 || status=$?
  if [ "$token" = good-token ]; then
    [ "$status" = 0 ] || fail "set-release-token failed for a good token"
    [ "$(cat "$secret")" = good-token ] || fail "set-release-token stored \"$(cat "$secret")\", not the token"
  else
    [ "$status" != 0 ] || fail "set-release-token passed a bad token"
    [ ! -e "$secret" ] || fail "set-release-token stored a bad token"
  fi
done
echo "check-release-token and set-release-token answer the fake GitHub as expected."
