# Every check CI runs is a recipe here, so `just ci` before pushing runs the same checks, apart from
# the CI jobs that need GitHub or another target, which "Check a change" in CONTRIBUTING.md lists.

# Only the formatter runs on this nightly. The policy crate builds on the stable toolchain in
# rust-toolchain.toml, and the lints on the nightly in lints/rust-toolchain.toml.
nightly := "nightly-2026-10-28"

# rustdoc warnings fail `just docs` and the doctests. Cargo reads `CARGO_ENCODED_RUSTDOCFLAGS`
# before `RUSTDOCFLAGS`, so a caller's setting of either, even an empty one, can't drop the flag.
export CARGO_ENCODED_RUSTDOCFLAGS := "-Dwarnings"

# clippy reads only the root `clippy.toml`, whatever other clippy configuration a directory holds.
export CLIPPY_CONF_DIR := justfile_directory()

# Lists the recipes.
default:
    @just --list

# Runs every check CI runs, apart from the jobs "Check a change" in CONTRIBUTING.md lists.
ci: fmt-check typos actionlint zizmor shear deny changelog release-test links pins lint dylint test docs lints-lint lints-comments lints-test templates

# Installs the pinned nightly rustfmt that `just fmt` uses.
install-nightly-rustfmt:
    rustup toolchain install {{ nightly }} --profile minimal --component rustfmt

# taplo reads `.taplo.toml`, which projects share and which excludes only `target/`, so these pass
# it the TOML files git doesn't ignore, which leaves out `lints/target/` too.
#
# Formats the Rust and TOML files.
fmt:
    cargo +{{ nightly }} fmt --all
    cargo +{{ nightly }} fmt --all --manifest-path lints/Cargo.toml
    cargo +{{ nightly }} fmt --all --manifest-path tests/musl-c/Cargo.toml
    cargo +{{ nightly }} fmt --all --manifest-path tests/installer/app/Cargo.toml
    git ls-files -z --cached --others --exclude-standard -- '*.toml' | xargs -0 taplo fmt

# Checks the Rust and TOML files are formatted.
fmt-check:
    cargo +{{ nightly }} fmt --all -- --check
    cargo +{{ nightly }} fmt --all --manifest-path lints/Cargo.toml -- --check
    cargo +{{ nightly }} fmt --all --manifest-path tests/musl-c/Cargo.toml -- --check
    cargo +{{ nightly }} fmt --all --manifest-path tests/installer/app/Cargo.toml -- --check
    git ls-files -z --cached --others --exclude-standard -- '*.toml' | xargs -0 taplo fmt --check

# Checks spelling in code and docs.
typos:
    typos

# actionlint skips the shell scripts without a word when shellcheck is missing, so this runs
# shellcheck first to fail instead.
#
# Checks the GitHub workflows and their shell scripts.
actionlint:
    shellcheck --version
    actionlint

# Audits the GitHub workflows for security problems.
zizmor:
    zizmor .github

# Checks every declared dependency is used.
shear:
    cargo shear

# Checks dependencies for advisories, licenses, bans and sources, and the lints' for advisories.
deny:
    cargo deny --locked check
    cargo deny --locked --manifest-path lints/Cargo.toml check advisories

# The newest heading must be `Unreleased`, the newest tag, or the version after it by the bump rule
# "Release trellis" in CONTRIBUTING.md gives for before 1.0; after it, any next version passes. A
# major bump always passes. CI's `Release` job tags the version this prints.
#
# Checks CHANGELOG.md's newest heading against the tags and prints its version.
changelog:
    #!/usr/bin/env bash
    set -euo pipefail
    fail() { echo "error: CHANGELOG.md: $1" >&2; exit 1; }
    heading=$(sed -n 's/^## //p' CHANGELOG.md | head -n 1)
    [ "$heading" != Unreleased ] || exit 0
    version='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
    [[ $heading =~ $version ]] \
      || fail "the newest heading, \"## $heading\", must be \"## Unreleased\" or a version, such as \"## v1.2.3\""
    latest=$(git tag --list 'v*' --sort=-version:refname | grep --extended-regexp "$version" | head -n 1) || true
    [ -n "$latest" ] || fail "found no vX.Y.Z tag to compare $heading with; fetch the tags with \`git fetch --tags\`"
    if [ "$heading" = "$latest" ]; then
      echo "$heading"
      exit 0
    fi
    if git rev-parse --quiet --verify "refs/tags/$heading" > /dev/null; then
      fail "the newest heading names $heading, which is older than the newest tag, $latest"
    fi
    [[ $latest =~ $version ]]
    major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}
    patch_bump="v$major.$minor.$((patch + 1))"
    minor_bump="v$major.$((minor + 1)).0"
    major_bump="v$((major + 1)).0.0"
    asks=$(awk '/^## / { if (seen) exit; seen = 1; next }
      /^### / { inside = ($0 == "### Action required in projects"); next }
      inside && /^([-*+]|[0-9]+\.) /' CHANGELOG.md)
    case "$heading" in
      "$major_bump") ;;
      "$minor_bump")
        [ -n "$asks" ] || [ "$major" != 0 ] \
          || fail "$heading bumps the minor version, but its \"Action required in projects\" lists nothing, so it must be $patch_bump" ;;
      "$patch_bump")
        [ -z "$asks" ] || [ "$major" != 0 ] \
          || fail "$heading bumps the patch version, but its \"Action required in projects\" asks something of projects, so it must be $minor_bump" ;;
      *) fail "$heading must follow $latest as $patch_bump, $minor_bump or $major_bump" ;;
    esac
    echo "$heading"

# CI's `Release` job runs this on `main` once every other job passed, with a GH_TOKEN that can
# write the repository's contents. It tags HEAD only if HEAD changed the version's CHANGELOG.md
# section and no commit has the tag. CI runs each commit on `main` separately, and a later commit's
# run can pass first; tagging it would release changes the notes don't list. GitHub refuses a tag
# that exists, so of two runs that race, the second skips. The tag is annotated, as the ones made
# by hand were, and the release's notes are the section. A rerun on the commit it tagged publishes
# the release a failed run missed, until a newer version is tagged; on any other commit it does
# nothing. That includes a commit whose run finished after a later commit's run tagged a newer
# version, while its own CHANGELOG.md names a version already tagged on another commit. A commit
# that names a version no commit has, such as one a later version overtook before its run tagged
# it, fails, so the missing release is noticed.
#
# Tags and releases the version `just changelog` prints.
release:
    #!/usr/bin/env bash
    set -euo pipefail
    head=$(git rev-parse HEAD)
    named=$(sed -n 's/^## //p' CHANGELOG.md | head -n 1)
    latest=$(git tag --list 'v*' --sort=-version:refname \
      | grep --extended-regexp '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' | head -n 1) || true
    released=$(git rev-parse --quiet --verify "refs/tags/$named^{commit}") || true
    if [ -n "$latest" ] && [ -n "$released" ] && [ "$released" != "$head" ] \
      && [ "$(git rev-parse "$latest^{commit}")" != "$head" ] && git merge-base --is-ancestor HEAD "$latest"; then
      echo "$named is released at $released, and $latest, tagged on a later commit, holds this one, so there's nothing to release."
      exit 0
    fi
    version=$(just changelog)
    if [ -z "$version" ]; then
      echo "CHANGELOG.md's newest heading is Unreleased, so there's nothing to release."
      exit 0
    fi
    section() { awk '/^## / { if (seen) exit; seen = 1 } seen'; }
    tagged=$(git rev-parse --quiet --verify "refs/tags/$version^{commit}") || true
    if [ -z "$tagged" ]; then
      if [ "$(section < CHANGELOG.md)" = "$(git show HEAD^:CHANGELOG.md 2> /dev/null | section)" ]; then
        echo "This commit didn't change the $version section in CHANGELOG.md, so it doesn't get the tag. To release $version, rerun CI on the commit that last changed the section, or change the section again."
        exit 0
      fi
      tag=$(gh api 'repos/{owner}/{repo}/git/tags' -f "tag=$version" -f "message=$version" \
        -f "object=$head" -f type=commit --jq .sha)
      if ! gh api 'repos/{owner}/{repo}/git/refs' -f "ref=refs/tags/$version" -f "sha=$tag" > /dev/null; then
        if gh api "repos/{owner}/{repo}/git/ref/tags/$version" > /dev/null 2>&1; then
          echo "Another run tagged $version first."
          exit 0
        fi
        exit 1
      fi
      echo "Tagged $head as $version."
    elif [ "$tagged" != "$head" ]; then
      echo "$version is already released at $tagged."
      exit 0
    fi
    if gh release view "$version" > /dev/null 2>&1; then
      echo "$version already has a GitHub release."
      exit 0
    fi
    notes=$(section < CHANGELOG.md | tail -n +2 | sed '/./,$!d')
    gh release create "$version" --verify-tag --title "$version" --notes "$notes"

# Checks `changelog` and `release` against a scratch repository and a fake gh.
release-test:
    tests/release/test.sh "{{ justfile() }}"

# The template's files link to files that exist only once a project is generated, so the
# template tests check those.
#
# Checks that every relative link and anchor in the Markdown files resolves.
links:
    lychee --offline --include-fragments --no-progress --exclude-path target --exclude-path lints/target --exclude-path template --exclude-path policy/tests/readme './**/*.md'

# Checks that the template pins the same toolchains as this repository and its lints, that the
# config files and workflows both keep are the same apart from the Advisories workflow's web UI
# step and what only a project with license notices runs, that both `[workspace.lints]` tables
# agree apart from the library-only lints, that the template's two lists of accepted licenses
# agree, that the versions template/dylint.toml.jinja says trellis read each trusted macro crate
# at are the ones tests/dependencies/setup.sh pins, that the Web UI guide in
# template/docs/maintaining.md.jinja writes bun.lock with the command web-install names in
# template/justfile.jinja, and that every tool and runner image has one version wherever it's pinned in a form the recipe reads,
# such as `name@version`. A value it can't find or read fails, as does a file it reads that's gone.
#
# Checks that pinned versions and the shared config files agree.
pins:
    #!/usr/bin/env bash
    set -euo pipefail
    # Sorts by byte: a locale that ignores punctuation could sort another tool between two versions
    # of one, and hide the pair.
    export LC_ALL=C
    # git writes its traces where its errors go, and a message from git grep fails the recipe.
    unset "${!GIT_TRACE@}"
    status=0
    # Prints what a sed script prints from a file, or fails when that's nothing.
    extract() {
      local found
      found=$(sed -n "$2" "$3") || true
      if [ -z "$found" ]; then
        echo "error: found no $1 in $3" >&2
        return 1
      fi
      printf '%s\n' "$found"
    }
    channel() { extract "toolchain channel" 's/^channel = "\(.*\)"$/\1/p' "$1"; }
    variable() { extract "$1 variable" "s/^$1 := \"\(.*\)\"$/\1/p" "$2"; }
    key() { extract "$1 key" "s/^$1 = \"=\{0,1\}\(.*\)\"$/\1/p" "$2"; }
    check() {
      if [ -z "$2" ] || [ -z "$3" ] || [ "$2" != "$3" ]; then
        echo "error: $1: ${2:-(none)} != ${3:-(none)}" >&2
        status=1
      fi
    }
    check "template/rust-toolchain.toml and rust-toolchain.toml" \
      "$(channel template/rust-toolchain.toml)" "$(channel rust-toolchain.toml)"
    check "lints_nightly in template/justfile.jinja and lints/rust-toolchain.toml" \
      "$(variable lints_nightly template/justfile.jinja)" "$(channel lints/rust-toolchain.toml)"
    lockfile() { extract "bun.lock command" "$1" "$2"; }
    check "the bun.lock command in template/docs/maintaining.md.jinja and template/justfile.jinja's web-install" \
      "$(lockfile 's/^ *`\(bun install [^`]*\)`, the command `just web-install` names.*/\1/p' template/docs/maintaining.md.jinja)" \
      "$(lockfile 's/.*Write it there with \(bun install [^.]*\) and commit it\..*/\1/p' template/justfile.jinja)"
    check "rust-version in template/Cargo.toml.jinja and Cargo.toml" \
      "$(key rust-version template/Cargo.toml.jinja)" "$(key rust-version Cargo.toml)"
    check "rust-version in Cargo.toml and the stable toolchain" \
      "$(key rust-version Cargo.toml)" "$(channel rust-toolchain.toml | cut -d. -f1-2)"
    check "dylint_linting and dylint_testing in lints/Cargo.toml" \
      "$(key dylint_linting lints/Cargo.toml)" "$(key dylint_testing lints/Cargo.toml)"
    check "dylint_linting in lints/Cargo.toml and the cargo-dylint the workflows install" \
      "$(key dylint_linting lints/Cargo.toml)" \
      "$(grep --only-matching --no-filename 'cargo-dylint@[0-9.]*' .github/workflows/ci.yml | head -n 1 | cut -d@ -f2)"
    check "nightly in template/justfile.jinja and justfile" \
      "$(variable nightly template/justfile.jinja)" "$(variable nightly justfile)"
    check "the licenses template/deny.toml.jinja allows and template/about.toml.jinja accepts" \
      "$(extract "allow list" 's/^allow = \(\[.*\]\)$/\1/p' template/deny.toml.jinja)" \
      "$(extract "accepted list" 's/^accepted = \(\[.*\]\)$/\1/p' template/about.toml.jinja)"
    # The lints' nightly must be newer than projects' stable, or code using an API that stable
    # just stabilized passes clippy and fails `just dylint`.
    lints_minor=$(sed -n 's/^clippy_utils = "=0\.1\.\([0-9]*\)"$/\1/p' lints/Cargo.toml)
    stable_minor=$(channel rust-toolchain.toml | sed -n 's/^1\.\([0-9]*\)\..*$/\1/p')
    if [ -z "$lints_minor" ] || [ -z "$stable_minor" ] || [ "$lints_minor" -le "$stable_minor" ]; then
      echo "error: clippy_utils in lints/Cargo.toml (0.1.${lints_minor:-?}) must be for a newer Rust than rust-toolchain.toml's 1.${stable_minor:-?}" >&2
      status=1
    fi
    if ! grep --quiet --fixed-strings "[\"+$(variable nightly template/justfile.jinja)\"]" template/CONTRIBUTING.md.jinja; then
      echo "error: template/CONTRIBUTING.md.jinja's rust-analyzer setting doesn't name the nightly in template/justfile.jinja" >&2
      status=1
    fi
    for file in rustfmt.toml clippy.toml .taplo.toml .editorconfig .gitattributes .config/nextest.toml \
      .github/workflows/pr-title.yml; do
      if ! cmp --quiet "$file" "template/$file"; then
        echo "error: $file differs from template/$file" >&2
        status=1
      fi
    done
    # The template's Advisories workflow adds a step only a project with a web UI gets.
    if ! diff .github/workflows/deny.yml \
      <(sed "/^{%- if web_ui %}$/,/^{%- endif %}$/d; /^{%- if kind != 'library' %}$/,/^{%- endif %}$/d
        s/{{{{ ',cargo-about@[^']*' if kind != 'library' else '' }}//" template/.github/workflows/deny.yml.jinja) >&2; then
      echo "error: .github/workflows/deny.yml differs from template/.github/workflows/deny.yml.jinja" >&2
      status=1
    fi
    # CI's `C code for musl` job checks the step that installs musl-gcc in a generated binary's
    # workflows, so its copy must match theirs.
    musl_step() {
      awk '/^      - name: Install musl-gcc$/ { inside = 1; print; next }
        inside && /^        / { print; next } { inside = 0 }' "$1" \
        | sed 's/{% raw %}//; s/{% endraw %}//'
    }
    for file in template/.github/workflows/ci.yml.jinja template/.github/workflows/build-release.yml.jinja; do
      if [ -z "$(musl_step "$file")" ] || ! diff <(musl_step .github/workflows/ci.yml) <(musl_step "$file") >&2; then
        echo "error: the musl-gcc step in .github/workflows/ci.yml differs from $file's" >&2
        status=1
      fi
    done
    # The template's lints tables add a block only a library gets.
    lints_tables() {
      awk '/^\[/ { inside = ($0 ~ /^\[workspace\.lints\./) } inside' "$1" \
        | sed "/^{%- if kind == 'library' %}$/,/^{%- endif %}$/d"
    }
    if ! diff <(lints_tables Cargo.toml) <(lints_tables template/Cargo.toml.jinja) >&2; then
      echo "error: [workspace.lints] in Cargo.toml differs from template/Cargo.toml.jinja's" >&2
      status=1
    fi
    # Each crate's comment in trusted_macro_crates names the version trellis read its macros at, the
    # one tests/dependencies/setup.sh pins, as `<macros>, X.Y.Z: <what they write>`. The standard
    # library's crates have no version. A `_impl` crate is pinned through its parent.
    audited=$(awk '
      /^trusted_macro_crates = \[$/ { inside = 1; next }
      inside && /^\]$/ { inside = 0 }
      !inside { next }
      !version && /^ *#.*, [0-9]+\.[0-9]+\.[0-9]+:/ {
        match($0, /[0-9]+\.[0-9]+\.[0-9]+/)
        version = substr($0, RSTART, RLENGTH)
      }
      /^ *"[a-z0-9_]+",$/ {
        gsub(/[ ",]/, "")
        if ($0 !~ /^(alloc|core|std)$/) print $0 "@" version
        version = ""
        next
      }
      !/^ *(#.*)?$/ {
        print "error: trusted_macro_crates in template/dylint.toml.jinja has a line pins cannot read: " $0 > "/dev/stderr"
        unreadable = 1
      }
      END { exit unreadable }
    ' template/dylint.toml.jinja) || status=1
    pinned=$(sed -n -e 's/^\([a-z_-]*\) = \(.*version = \)\{0,1\}"=\([0-9.]*\)".*$/\1@\3/p' \
      -e 's/.*--package \([a-z_-]*\) --precise \([0-9.]*\)$/\1@\2/p' tests/dependencies/setup.sh)
    if [ -z "$audited" ]; then
      echo "error: found no crates besides alloc, core and std in template/dylint.toml.jinja's trusted_macro_crates" >&2
      status=1
    fi
    for crate_version in $audited; do
      crate=${crate_version%@*} version=${crate_version#*@}
      if [ -z "$version" ]; then
        echo "error: $crate in template/dylint.toml.jinja's trusted_macro_crates has no audited version; its comment must say \`<macros>, X.Y.Z: <what they write>\`" >&2
        status=1
        continue
      fi
      check "the version $crate is audited at in template/dylint.toml.jinja and tests/dependencies/setup.sh" \
        "$version" "$(grep "^${crate%_impl}@" <<< "$pinned" | cut -d@ -f2)"
    done
    # Every file git has or would add is scanned but these: CHANGELOG.md names old versions, and
    # policy/ and the web starter's lockfile hold npm package keys, which aren't pins.
    scan=(. ':!CHANGELOG.md' ':!policy/' ':!*bun.lock.jinja')
    errors=$(mktemp)
    # Overrides each setting that changes what git grep prints, or stops it, and reads every file
    # as text, whatever a git attribute calls it, so an untracked binary file is read too: ignore
    # or delete one that fails here. git grep exits as usual for a file it can't read, so a
    # message from it fails too.
    git_grep() {
      local found=0
      git grep --untracked --no-recurse-submodules --no-color --no-column -a -z -E "$@" 2> "$errors" || found=$?
      if [ -s "$errors" ] || [ "$found" -gt 1 ]; then
        cat "$errors" >&2
        return 2
      fi
      return "$found"
    }
    # A version is one or more numbers, with a pre-release suffix and build metadata if it has them.
    version='([0-9]+([.][0-9]+)*(-[0-9A-Za-z-]+([.][0-9A-Za-z-]+)*)?([+][0-9A-Za-z-]+([.][0-9A-Za-z-]+)*)?)'
    matches=$(mktemp)
    trap 'rm -f "$matches" "$errors"' EXIT
    # Prints `<tool>@<version> <file>:<line>` for each match of an extended regex whose first group
    # is the version, in the scanned files or the files named after it, and fails when there's none.
    pinned_as() {
      local tool=$1 regex=$2 where="the scanned files" file line match found=""
      shift 2
      if [ $# -gt 0 ]; then where=$*; else set -- "${scan[@]}"; fi
      git_grep --line-number --only-matching "$regex" -- "$@" > "$matches" || [ $? -eq 1 ] || return 1
      while IFS= read -r -d '' file && IFS= read -r -d '' line && IFS= read -r match; do
        if ! [[ $match =~ ^$regex$ ]]; then
          echo "error: $file:$line: can't read a $tool version in $match" >&2
          return 1
        fi
        found+="$tool@${BASH_REMATCH[1]} $file:$line"$'\n'
      done < "$matches"
      if [ -z "$found" ]; then
        echo "error: found no $tool version, as ${regex//"$version"/<version>} matches, in $where" >&2
        return 1
      fi
      printf '%s' "$found"
    }
    # Prints `release-plz@<version> <file>:<line>` for the `version:` input of each step that runs
    # the release-plz action, and fails on a step without one or when no file runs it.
    release_plz() {
      git_grep --files-with-matches 'uses: *release-plz/action@' -- "${scan[@]}" > "$matches" || [ $? -eq 1 ] || return 1
      if ! [ -s "$matches" ]; then
        echo "error: found no workflow that runs the release-plz action" >&2
        return 1
      fi
      xargs -0 awk -v version="^$version" '
        function unpinned() {
          print "error: " step ": the release-plz action runs without a version: input" > "/dev/stderr"
          failed = 1
        }
        FNR == 1 && step { unpinned(); step = "" }
        /uses: *release-plz\/action@/ { if (step) unpinned(); step = FILENAME ":" FNR; next }
        step && match($0, /^ *version: *["\047]?/) {
          rest = substr($0, RLENGTH + 1)
          if (match(rest, version)) {
            print "release-plz@" substr(rest, 1, RLENGTH) " " FILENAME ":" FNR
          } else {
            print "error: " FILENAME ":" FNR ": can'"'"'t read the release-plz version in " $0 > "/dev/stderr"
            failed = 1
          }
          step = ""
          next
        }
        step && /^ *- / { unpinned(); step = "" }
        END { if (step) unpinned(); exit failed }
      ' < "$matches"
    }
    versions=""
    collect() {
      local found
      if found=$("$@"); then
        versions+=$found$'\n'
      else
        status=1
      fi
    }
    collect pinned_as tool "\b([a-z][a-z0-9-]*@v?$version)\b"
    collect release_plz
    collect pinned_as shellcheck "\[shellcheck\]\([^)]*\) $version"
    collect pinned_as bun "bun-version: *[\"']?$version"
    collect pinned_as bun "docker\.io/oven/bun:$version-slim@"
    collect pinned_as bun "\[Bun\]\([^)]*\) $version"
    # A runner image's ARM64 label is its x86-64 one with `-arm`, apart from Windows on ARM64's.
    # `latest` is a version like any other, so it conflicts with a numbered label.
    collect pinned_as runner "\b((ubuntu|macos|windows)-([0-9][0-9.]*|latest)(-arm)?)\b"
    # Inno Setup's pin is its x64 installer's URL, which names the version in the release's tag and
    # in the file name; tests/installer/run.sh's Inno Setup 6 has no `-x64`, so it isn't the pin.
    collect pinned_as innosetup "/is-[0-9_]+/innosetup-$version-x64\.exe"
    inno_urls=$(git_grep -h -o 'is-[0-9_]+/innosetup-[0-9.]+(-x64)?\.exe' -- "${scan[@]}") || [ $? -eq 1 ] || status=1
    for url in $inno_urls; do
      tag=$(sed -E 's|^is-([0-9_]+)/.*$|\1|' <<< "$url" | tr _ .)
      if [ "$tag" != "$(sed -E 's|^.*/innosetup-([0-9.]+)(-x64)?\.exe$|\1|' <<< "$url")" ]; then
        echo "error: the Inno Setup URL ending $url names two versions" >&2
        status=1
      fi
    done
    versions=$(sed -E '/^$/d; s/^tool@//; s/@v/@/; s/^typos-cli@/typos@/; s/^taplo-cli@/taplo@/
      s/^runner@windows-([^ ]*)-arm /windows-arm-runner@\1 /; t
      s/^runner@(ubuntu|macos)-([^ ]*)-arm /\1-runner@\2 /; t
      s/^runner@([a-z]+)-/\1-runner@/' <<< "$versions" | sort --unique)
    for tool in $(cut -d' ' -f1 <<< "$versions" | sort --unique | cut -d@ -f1 | uniq --repeated); do
      echo "error: $tool is pinned to more than one version:" >&2
      grep "^$tool@" <<< "$versions" | sed -E 's/^[^@]*@([^ ]*) (.*):([0-9]+)$/\1\t\2\t\3/' \
        | sort -t $'\t' -k 1,1 -k 2,2 -k 3,3n | awk -F '\t' '
          NR == 1 || $1 "" != version "" { if (NR > 1) print ""; printf "  %s in %s:%s", $1, $2, $3; version = $1; file = $2; next }
          $2 != file { printf ", %s:%s", $2, $3; file = $2; next }
          { printf ",%s", $3 }
          END { print "" }
        ' >&2
      status=1
    done
    exit "$status"

# Runs clippy on every target with warnings as errors.
lint:
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

# Runs the custom lints over `policy/`, warnings as errors.
dylint: (_dylint "-D warnings")

[private]
_dylint $DYLINT_RUSTFLAGS:
    cargo dylint --all --fail-on-no-libraries -- --locked --workspace --all-targets --all-features

# Runs the tests, then the doctests, which nextest does not run.
# The flags run ignored tests too, so `#[ignore]` can't skip a test here or in CI even where
# `disabled_code` doesn't report it.
test:
    cargo nextest run --locked --workspace --all-features --run-ignored all
    cargo test --locked --workspace --doc --all-features -- --include-ignored

# Builds the docs, private items included.
docs:
    cargo doc --locked --workspace --no-deps --document-private-items --all-features

# Runs clippy on the custom lints, warnings as errors.
[working-directory('lints')]
lints-lint:
    cargo clippy --locked --all-targets -- -D warnings

# Only `code_comment` runs: the lints' code implements rustc's lint traits through rustc's macros,
# which the other lints would flag. `forbid` keeps an `allow` in the code from turning it off.
#
# Runs `code_comment` over the custom lints' own code, so they follow the rule they enforce.
[working-directory('lints')]
lints-comments:
    DYLINT_TOML="$(cat ../dylint.toml)" \
      DYLINT_RUSTFLAGS="-A warnings -A suppression_policy -A missing_workspace_lints -F code_comment" \
      cargo dylint --path . -- --locked --all-targets

# Runs the custom lints' UI tests.
[working-directory('lints')]
lints-test:
    cargo test --locked

# Runs `template` for each kind, then `shapes`, `outlines`, `release-gate`, `dependencies`, `update`
# and the extras checks.
templates: (template "binary") (template "binary" "ci" "false" "false" "" "false" "desktop") (template "binary" "ci" "false" "false" "" "false" "terminal") (template "binary" "ci" "false" "true" "" "false" "web,cli") (template "service") (template "service" "ci" "false" "true") (template "library") (template "library" "ci" "true") shapes outlines release-gate dependencies update build-extras image-extras release-extras checker-install

# Checks that every exclusion in copier.yml renders with only the answers an older trellis asked,
# as Copier renders them when a project updates from it. Then copies the working tree into a
# scratch repository as two tagged trellis versions, generates a library from the first, and
# edits it the way an owner would: its code and changelog, and a new crate and docs page with their
# lines in CONTRIBUTING.md, Cargo.toml, release-plz.toml and README.md.
# Then it changes the template's scaffold, a rule file, a docs page, the README and lines near the
# owner's, and runs `copier update` to the second. The update must merge each file with no conflict
# markers, keep the owner's lines, bring v2's, and pass `trellis-policy check`; the owner's code,
# changelog and README must not move. The first writes readme_sections and the README's first
# section as trellis 0.19 did, so after the update `trellis-policy check` must report only that the
# README's sections don't match, and pass once the owner renames the section to `Quick start`. A
# terminal app generated from the first, whose owner edits its README and screenshots page,
# updates to the second with both byte for byte the same. Then it makes the project private and public again with
# `--vcs-ref=:current:`, which must keep v2 and the owner's edits, and publishes the crate under
# each license, which must deliver only that license's files to the crate's directory and fail
# `trellis-policy check` on the owner's crate until the owner swaps in the root's there too, from
# no files and from the other license's; each of these answer changes must then pass
# `trellis-policy check`. Last, it generates a library with no
# license and publishes it: the update must refuse `publish_crate=true` without a license, and
# with `license=MIT` it must fail `trellis-policy check` until the crate inherits the license,
# then pass it and `trellis-policy rule-changes` as one rule change.
#
# Checks that `copier update` moves the rules and keeps the owner's code and edits.
update:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    commit() { git -c user.name=trellis -c user.email=trellis@localhost "$@"; }
    copier() { uv tool run copier@9.19.1 "$@"; }
    policy() { cargo run --locked --quiet --manifest-path "$trellis/Cargo.toml" --package trellis-policy -- "$@"; }
    uv run --quiet --no-project --with jinja2==3.1.6 --with pyyaml==6.0.3 python -I \
      tests/exclusions/check.py copier.yml
    mkdir "$out/trellis"
    tar --create --exclude=./.git --exclude=./target --exclude=./lints/target --file=- . \
      | tar --extract --directory="$out/trellis" --file=-
    cd "$out/trellis"
    # v1 writes the README's first section and readme_sections as trellis 0.19 did, so the update
    # moves readme_sections to Quick start while the owner's README keeps `## Use it`.
    sed -i.bak 's/^readme_sections = \["Quick start", /readme_sections = ["Use it", /' template/Cargo.toml.jinja
    sed -i.bak 's/^## Quick start$/## Use it/' template/README.md.jinja
    rm template/Cargo.toml.jinja.bak template/README.md.jinja.bak
    grep --quiet '^readme_sections = \["Use it", ' template/Cargo.toml.jinja || { echo "error: v1 kept the new readme_sections" >&2; exit 1; }
    git init --quiet --initial-branch main
    git add --all
    commit commit --quiet --message v1
    commit tag --annotate --message v1 v1
    copier copy --quiet --defaults --vcs-ref v1 --data project_name=demo \
      --data "description=A demo library." --data kind=library "$out/trellis" "$out/demo"
    cd "$out/demo"
    git init --quiet --initial-branch main
    git add --all
    commit commit --quiet --message "chore: generate from trellis"
    fail() { echo "error: $1" >&2; exit 1; }
    has() { grep --quiet --line-regexp --fixed-strings -- "$2" "$1"; }
    # Runs an update with the arguments after $2 that must fail with the message $2 and change no
    # file; $1 says what a success did.
    refuse() {
      local done=$1 message=$2 refusal
      shift 2
      if refusal=$(copier update --quiet --defaults --vcs-ref=:current: "$@" 2>&1); then
        fail "$done"
      fi
      grep --quiet --fixed-strings -- "$message" <<< "$refusal" || fail "the update refused for another reason: $refusal"
      [ -z "$(git status --porcelain)" ] || fail "the refused update changed files"
    }
    # Replaces the line of the file $1 that is exactly $2 with $3, in which `\n` starts a new line.
    swap() {
      awk -v old="$2" -v new="$3" '!done && $0 == old { print new; done = 1; next } { print } END { exit !done }' \
        "$1" > "$1.new" || fail "$1 has no line $2"
      mv "$1.new" "$1"
    }
    lib=crates/demo/src
    printf '//! Mine.\n\nmod parse;\n\npub use parse::parse;\n' > "$lib/lib.rs"
    printf '/// Parses.\n#[must_use]\npub fn parse() -> u8 {\n    1\n}\n' > "$lib/parse.rs"
    rm "$lib/greeting.rs"
    echo "- An owner's line." >> CHANGELOG.md
    git add --all
    commit commit --quiet --message "feat: parse"
    # The owner adds a crate and a docs page as CONTRIBUTING.md's "Layout", "Product boundaries",
    # "Docs" and "New crates" sections say, in the trellis files that an update merges.
    mkdir -p crates/demo_text/src
    sed 's/^name = "demo"$/name = "demo_text"/; s/^description = .*$/description = "Text helpers."/' \
      crates/demo/Cargo.toml > crates/demo_text/Cargo.toml
    printf '//! Text helpers.\n' > crates/demo_text/src/lib.rs
    printf '# Formats\n\nThe text formats demo parses, for anyone who calls it.\n' > docs/formats.md
    cargo metadata --quiet --offline --format-version 1 > /dev/null
    library_row='| `crates/demo` | The library. |'
    row='| `crates/demo_text` | Text helpers for the parser. |'
    boundaries='builds past them. Add one only when the owner decides it.'
    boundary='- It reads text and never writes files.'
    design='- [Design](docs/design.md): the goals, the architecture, and the decisions behind them.'
    index='- [Formats](docs/formats.md): the text formats it parses.'
    swap CONTRIBUTING.md "$library_row" "$library_row\n$row"
    swap CONTRIBUTING.md "$boundaries" "$boundaries\n\n$boundary"
    swap Cargo.toml '    "docs/design.md",' '    "docs/design.md",\n    "docs/formats.md",'
    swap Cargo.toml 'demo = []' 'demo = []\ndemo_text = []'
    swap release-plz.toml 'changelog_include = []' 'changelog_include = ["demo_text"]'
    swap README.md "$design" "$index\n$design"
    printf '\nThe owner writes the README.\n' >> README.md
    git add --all
    commit commit --quiet --message "feat: add demo_text and a formats page"
    cp README.md "$out/README.md"
    owner=(CONTRIBUTING.md "$row" CONTRIBUTING.md "$boundary" Cargo.toml '    "docs/formats.md",' Cargo.toml 'demo_text = []'
      release-plz.toml 'changelog_include = ["demo_text"]' README.md "$index")
    kept() {
      for ((i = 0; i < ${#owner[@]}; i += 2)); do
        has "${owner[i]}" "${owner[i + 1]}" || fail "$1 lost the owner's line in ${owner[i]}: ${owner[i + 1]}"
      done
    }
    cd "$out/trellis"
    cp "$trellis/template/Cargo.toml.jinja" "$trellis/template/README.md.jinja" template/
    scaffold="template/crates/{% if kind == 'library' %}{{{{ crate_name }}{% endif %}/src"
    sed -i.bak 's/Hello, /Hi, /' "$scaffold/greeting.rs" && rm "$scaffold/greeting.rs.bak"
    printf '/// New.\npub fn added() {}\n' > "$scaffold/added.rs"
    echo "# A rule a later trellis adds." >> template/rustfmt.toml
    printf '# Added\n\nA page a later trellis adds.\n' > template/docs/added.md
    # Each change sits within Copier's three lines of patch context of an owner's edit, so the
    # patch doesn't apply and Copier merges the file three ways. None changes the line right next
    # to an owner's line, which git merges as a conflict.
    rows='Add a row for each crate you add, in the rule change that allows it.'
    asking='builds past them. Only the owner adds one.'
    edges='# crates, is a rule change. A later trellis says more here.'
    release='# A line a later trellis adds.'
    added='- [Added](docs/added.md): what a later trellis adds.'
    maintain='- [Maintain](docs/maintaining.md): the owner'"'"'s repository settings, releases and trellis updates.'
    swap template/CONTRIBUTING.md.jinja \
      'Add a row for each crate you add, in the same pull request as the rule change that allows it.' "$rows"
    swap template/CONTRIBUTING.md.jinja "$boundaries" "$asking"
    swap template/Cargo.toml.jinja '    "docs/design.md",' '    "docs/added.md",\n    "docs/design.md",'
    swap template/Cargo.toml.jinja '# crates, is a rule change.' "$edges"
    swap template/release-plz.toml.jinja 'changelog_update = true' "$release\nchangelog_update = true"
    swap template/README.md.jinja "$design" "$design\n- [Stray](docs/stray.md): a line the README never gets."
    swap template/CONTRIBUTING.md.jinja "$maintain" "$maintain\n$added"
    echo "A line a later trellis adds." >> template/docs/usage.md.jinja
    git add --all
    commit commit --quiet --message v2
    commit tag --annotate --message v2 v2
    cd "$out/demo"
    copier update --quiet --defaults --vcs-ref v2
    if grep --recursive --quiet '^<<<<<<<' --exclude-dir=.git .; then
      grep --recursive --line-number '^<<<<<<<' --exclude-dir=.git . >&2
      fail "the update left conflict markers"
    fi
    kept "the update"
    trellis_lines=(CONTRIBUTING.md "$rows" CONTRIBUTING.md "$asking" Cargo.toml '    "docs/added.md",' Cargo.toml "$edges"
      release-plz.toml "$release" CONTRIBUTING.md "$added")
    for ((i = 0; i < ${#trellis_lines[@]}; i += 2)); do
      has "${trellis_lines[i]}" "${trellis_lines[i + 1]}" \
        || fail "the update didn't bring v2's line to ${trellis_lines[i]}: ${trellis_lines[i + 1]}"
    done
    ! has CONTRIBUTING.md "$boundaries" || fail "the update kept v1's Product boundaries sentence"
    cmp --quiet README.md "$out/README.md" || fail "the update changed the owner's README.md"
    # The README keeps `## Use it` while readme_sections moved, so `trellis-policy check` names the
    # mismatch, as the changelog's action item says, until the owner renames the section. Under
    # GitHub Actions the checker writes each error as an `::error::` annotation.
    if policy check > "$out/sections.txt" \
      || [ "$(grep --count -E '^(error: |::error::)' "$out/sections.txt")" != 1 ] \
      || ! grep --quiet -E '^(error: |::error::)\[docs_layout\] README.md: The readme'"'"'s `##` sections are `Use it`, `Documentation`, `License`, but `readme_sections` in `\[workspace.metadata.policy\]` lists `Quick start`, `Documentation`, `License`\.' "$out/sections.txt"; then
      cat "$out/sections.txt" >&2
      fail "trellis-policy check didn't report only the README's old section after the update"
    fi
    swap README.md '## Use it' '## Quick start'
    policy check
    grep --quiet '^_commit: v2$' .copier-answers.yml || fail "the answers don't record v2"
    tail -n 1 rustfmt.toml | grep --quiet 'A rule a later trellis adds' || fail "rustfmt.toml didn't move"
    [ -f docs/added.md ] || fail "the new docs page wasn't added"
    grep --quiet '^//! Mine.$' "$lib/lib.rs" || fail "the owner's lib.rs was rewritten"
    [ ! -e "$lib/greeting.rs" ] || fail "the deleted greeting.rs came back"
    [ ! -e "$lib/added.rs" ] || fail "a new scaffold file was added to the owner's crate"
    tail -n 1 CHANGELOG.md | grep --quiet "An owner's line" || fail "CHANGELOG.md was rewritten"
    # A terminal app's screenshots page is its owner's too.
    copier copy --quiet --defaults --vcs-ref v1 --data project_name=screens \
      --data "description=A demo terminal app." --data kind=binary --data interface=terminal \
      "$out/trellis" "$out/screens"
    (
      cd "$out/screens"
      git init --quiet --initial-branch main
      printf '\nThe owner writes the README.\n' >> README.md
      printf '\nThe owner writes the screenshots page.\n' >> docs/usage.md
      git add --all
      commit commit --quiet --message "docs: write the README and the screenshots page"
      mkdir "$out/screens-before"
      cp README.md docs/usage.md "$out/screens-before/"
      copier update --quiet --defaults --vcs-ref v2
      if grep --recursive --quiet '^<<<<<<<' --exclude-dir=.git .; then fail "the terminal app's update left conflict markers"; fi
      for file in README.md docs/usage.md; do
        cmp --quiet "$file" "$out/screens-before/${file##*/}" || fail "the update changed the owner's $file"
      done
      has CONTRIBUTING.md "$added" || fail "the terminal app's update didn't bring v2's line to CONTRIBUTING.md"
    )
    for visibility in private public; do
      git add --all
      commit commit --quiet --allow-empty --message "chore: make the repository $visibility"
      copier update --quiet --defaults --vcs-ref=:current: --data "visibility=$visibility"
      grep --quiet "^visibility: $visibility$" .copier-answers.yml || fail "the answers don't record $visibility"
      grep --quiet '^_commit: v2$' .copier-answers.yml || fail "changing the visibility moved trellis off v2"
      advisories=$(grep --count 'security/advisories/new' SECURITY.md || true)
      [ "$advisories" = "$([ "$visibility" = public ] && echo 1 || echo 0)" ] \
        || fail "SECURITY.md doesn't match a $visibility repository"
      grep --quiet '^//! Mine.$' "$lib/lib.rs" || fail "making it $visibility rewrote the owner's lib.rs"
      if grep --recursive --quiet '^<<<<<<<' --exclude-dir=.git .; then fail "making it $visibility left conflict markers"; fi
      kept "making it $visibility"
      policy check
      if [ "$visibility" = private ]; then
        git add --all
        commit commit --quiet --allow-empty --message "chore: try to publish the private crate"
        printf 'publish_crate: true\nlicense: MIT\n' > "$out/publish.yml"
        refuse "the update published a private library" 'a private library can'"'"'t publish one' \
          --data-file "$out/publish.yml"
      fi
    done
    for license in 'MIT OR Apache-2.0' MIT; do
      git add --all
      commit commit --quiet --allow-empty --message "chore: publish the crate under $license"
      copier update --quiet --defaults --vcs-ref=:current: --data publish_crate=true --data "license=$license"
      # The update leaves README.md alone, so the owner links the new license files there, which
      # `trellis-policy check` requires.
      dual='Released under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at'
      if [ "$license" = MIT ]; then
        swap README.md "$dual" 'Released under the [MIT License](LICENSE).'
        swap README.md 'your option.' ''
      else
        swap README.md 'Released under the [MIT License](LICENSE).' "$dual\nyour option."
      fi
      if [ "$license" = MIT ]; then
        delivered=(LICENSE) removed=(LICENSE-MIT LICENSE-APACHE)
      else
        delivered=(LICENSE-MIT LICENSE-APACHE) removed=(LICENSE)
      fi
      for file in "${delivered[@]}"; do
        cmp --quiet "$file" "crates/demo/$file" || fail "publishing under $license didn't deliver crates/demo/$file"
      done
      for file in "${removed[@]}"; do
        [ ! -e "crates/demo/$file" ] || fail "publishing under $license left crates/demo/$file"
      done
      grep --quiet '^//! Mine.$' "$lib/lib.rs" || fail "publishing under $license rewrote the owner's lib.rs"
      [ ! -e "$lib/greeting.rs" ] || fail "publishing under $license brought back greeting.rs"
      [ ! -e "$lib/added.rs" ] || fail "publishing under $license added a scaffold file to the owner's crate"
      if grep --recursive --quiet '^<<<<<<<' --exclude-dir=.git .; then fail "publishing under $license left conflict markers"; fi
      kept "publishing under $license"
      # demo_text, which the owner added, keeps the license files it had: none when first
      # published, and then the dual license's, which aren't `LICENSE`. So `trellis-policy check`
      # fails until the owner swaps in the root's files, as the docs say.
      if policy check > "$out/license.txt" \
        || ! grep --quiet "\[license_files\] crates/demo_text/Cargo.toml: \`demo_text\` is published under \`$license\`, but \`crates/demo_text/\` lacks \`${delivered[0]}\`" "$out/license.txt"; then
        cat "$out/license.txt" >&2
        fail "trellis-policy check passed a published crate without the files of $license"
      fi
      rm -f crates/demo_text/LICENSE crates/demo_text/LICENSE-MIT crates/demo_text/LICENSE-APACHE
      cp "${delivered[@]}" crates/demo_text/
      policy check
    done
    git add --all
    commit commit --quiet --message "chore: publish the crate"
    refuse "the update made a published library's repository private" \
      "a published library's repository stays public" --data visibility=private
    refuse "the update took the license off a published library" \
      'crates.io needs a license for a published crate' --data license=none
    copier update --quiet --defaults --vcs-ref=:current: --data visibility=private --data publish_crate=false
    grep --quiet '^visibility: private$' .copier-answers.yml || fail "the answers don't record private"
    grep --quiet '^publish_crate: false$' .copier-answers.yml || fail "making it private kept publishing"
    [ ! -e crates/demo/LICENSE ] || fail "making it private left crates/demo/LICENSE"
    kept "making it private and unpublished"
    policy check
    git add --all
    commit commit --quiet --message "chore: publish the crate again"
    copier update --quiet --defaults --vcs-ref=:current: --data visibility=public --data publish_crate=true
    git add --all
    commit commit --quiet --message "chore: unlicense the crate"
    copier update --quiet --defaults --vcs-ref=:current: --data license=none --data publish_crate=false
    grep --quiet '^license: none$' .copier-answers.yml || fail "the answers don't record no license"
    grep --quiet '^publish_crate: false$' .copier-answers.yml || fail "unlicensing it kept publishing"
    for file in LICENSE crates/demo/LICENSE; do
      [ ! -e "$file" ] || fail "unlicensing it left $file"
    done
    kept "unlicensing it"
    # The README is the owner's, so its License section goes by hand, as readme_sections did.
    awk '/^## License$/ { skip = 1 } !skip { print }' README.md > README.md.new && mv README.md.new README.md
    printf '%s\n' "$(cat README.md)" > README.md.new && mv README.md.new README.md
    sed -i.bak '/^license.workspace = true$/d' crates/demo/Cargo.toml crates/demo_text/Cargo.toml
    rm crates/demo/Cargo.toml.bak crates/demo_text/Cargo.toml.bak crates/demo_text/LICENSE
    policy check
    policy rule-changes HEAD
    copier copy --quiet --defaults --vcs-ref v2 --data project_name=unlicensed \
      --data "description=An unlicensed library." --data kind=library --data license=none \
      "$out/trellis" "$out/unlicensed"
    cd "$out/unlicensed"
    git init --quiet --initial-branch main
    git add --all
    commit commit --quiet --message "chore: generate from trellis"
    refuse "the update published a library with no license" \
      'crates.io needs a license for a published crate' --data publish_crate=true
    copier update --quiet --defaults --vcs-ref=:current: --data publish_crate=true --data license=MIT
    grep --quiet '^license: MIT$' .copier-answers.yml || fail "the answers don't record the MIT license"
    if policy check > "$out/unlicensed.txt"; then
      fail "trellis-policy check passed a crate that doesn't inherit the workspace license"
    fi
    grep --quiet 'license.workspace = true' "$out/unlicensed.txt" \
      || fail "trellis-policy check failed for another reason: $(cat "$out/unlicensed.txt")"
    swap crates/unlicensed/Cargo.toml 'publish.workspace = true' 'publish.workspace = true\nlicense.workspace = true'
    printf '\n## License\n\nReleased under the [MIT License](LICENSE).\n' >> README.md
    policy check
    policy rule-changes HEAD
    echo "copier update moved the rules and kept the owner's code and edits."

# Generates a project of one kind, `binary`, `service` or `library`, from the working tree, with
# the lints and checker from this checkout, and runs `recipes` in it, `ci` by default. `true` as
# `publish` makes a library a published one, and `true` as `web` gives a binary or service a web UI.
# `interfaces`, such as `desktop` or `web,cli`, answers `interface` with the first and
# `other_interfaces` with the rest, in place of their defaults. After `ci`,
# `just policy-release` must fail on the README outline's prompts. After `ci`, a binary on Linux
# runs its release build's steps for this machine's target, whose archive must hold
# the program, its license files and notices, and a web UI's build and notices only when it has
# one. After `ci`, a service also builds and runs its image when Docker is
# installed, under umask 077 with its license files 0600, and the image's user must be able to read
# them. A web UI's build and license notices must be in it too, and its install must refuse a
# missing bun.lock and a package that provides a `bun` command, and read the app's bunfig.toml. A
# library's `just dylint` must delete a lint library built from another trellis commit and keep one
# built from this one. After `ci`, an about.toml entry for a crate the graph doesn't hold must pass
# `just notices` and fail `just stale-licenses`. With a web UI, an npm package under a license
# `accepted` lacks must fail `just notices` until web-licenses.toml accepts it for that package,
# and an entry there for a package not installed must pass `just notices` and fail
# `just stale-licenses`. A deploy kit in the root `deploy/` and one in the first crate's `deploy/`
# must pass `just policy` and `just rule-changes` beside a code change, the root kit printing
# nothing more than no kit, while a script shellcheck faults in either fails `just policy`, and so
# does the `version` that `cargo add --path` gives a binary's or service's dependency on its other
# crate, a `release-plz.toml` that would write that crate's changelog into it, a crate's own
# `[package] version`, and a missing `release-plz.toml`. A published library's crate without its
# `LICENSE` fails `just policy` and `just package`. A `target` runs `test` and `build-release` for
# that target, as a project's CI does for the target a release ships.
#
# Generates a project of one kind and runs its checks.
template kind recipes="ci" publish="false" web="false" target="" installer="false" interfaces="":
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ replace(justfile_directory(), '\', '/') }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    # Git Bash on Windows: a `C:/...` path means the same directory to bash, Copier and Cargo.
    if command -v cygpath > /dev/null; then
      out=$(cygpath --mixed "$out")
    fi
    project="$out/demo-{{ kind }}"
    interface_answer="" others_answer=""
    if [ -n "{{ interfaces }}" ]; then
      interfaces="{{ interfaces }}"
      interface_answer="interface=${interfaces%%,*}"
      others=""
      [[ $interfaces != *,* ]] || others=${interfaces#*,}
      others_answer="other_interfaces=[$others]"
    fi
    installer_answer=""
    if [ "{{ installer }}" = true ]; then
      # The release answer tests/installer/run.sh expects: an installer and a portable program for
      # the target.
      target="{{ target }}"
      installer_answer="release={files: {$target: [\"demo-binary-{version}-setup.exe\", \"demo-binary-{version}-portable.exe\"]}, installer: {$target: {app_id: EBAC94F7-A24E-431F-B4F6-8E9D669E9E53, file: \"demo-binary-{version}-setup.exe\", name: Demo Binary}}}"
    fi
    uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
      --data project_name=demo-{{ kind }} \
      --data "description=A demo {{ kind }} that trellis generates for its tests." \
      --data kind={{ kind }} \
      --data trellis_path="$trellis" \
      $( [ "{{ publish }}" = true ] && echo "--data publish_crate=true --data license=MIT" ) \
      $( [ "{{ web }}" = true ] && echo "--data web_ui=true" ) \
      ${installer_answer:+--data "$installer_answer"} \
      ${interface_answer:+--data "$interface_answer"} ${others_answer:+--data "$others_answer"} \
      "$trellis" "$project"
    cd "$project"
    git init --quiet --initial-branch main
    git add --all
    git -c user.name=trellis -c user.email=trellis@localhost commit --quiet --message "chore: generate from trellis"
    if [ -n "{{ target }}" ]; then
      rustup target add "{{ target }}"
    fi
    for recipe in {{ recipes }}; do
      if [ -n "{{ target }}" ] && { [ "$recipe" = test ] || [ "$recipe" = build-release ]; }; then
        CARGO_BUILD_TARGET="{{ target }}" just "$recipe"
      else
        just "$recipe"
      fi
    done
    if [ "{{ installer }}" = true ]; then
      "$trellis/tests/installer/run.sh" "$project" "{{ target }}"
    fi
    if [ "{{ recipes }}" = ci ]; then
      # `ci` passed with the README an outline, but a release must fail on its prompts.
      if just policy-release > "$out/release.txt" 2>&1; then
        cat "$out/release.txt" >&2
        echo "error: just policy-release passed a README outline that still holds its prompts" >&2
        exit 1
      fi
      grep --quiet --extended-regexp '^(error: |::error::)\[readme\] README.md: Lines [0-9, ]* hold prompts of the outline' "$out/release.txt" \
        || { cat "$out/release.txt" >&2; echo "error: just policy-release failed, but not on the README's prompts" >&2; exit 1; }
      # `ci` ran as a new project's first run does, with no remote branch to compare with. With
      # one, the checks must have left no change behind.
      git update-ref refs/remotes/origin/main HEAD
      just rule-changes
      # A deploy kit, in the root `deploy/` or a crate's `deploy/`, is code that `just policy`
      # shellchecks: a kit change passes beside a code change, a passing root kit prints nothing
      # more than no kit does, and a script shellcheck faults fails in either kit.
      source=$(git ls-files 'crates/*/src/*.rs' | head -n 1)
      crate_kit="${source%%/src/*}/deploy"
      just policy 2> "$out/policy.err" > "$out/no-kit.txt" || { cat "$out/policy.err" >&2; exit 1; }
      mkdir deploy
      printf '#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p "$HOME/.local/share/demo"\n' > deploy/install.sh
      printf '[Unit]\nDescription=demo\n' > deploy/demo.service
      just policy 2> "$out/policy.err" > "$out/root-kit.txt" || { cat "$out/policy.err" >&2; exit 1; }
      diff "$out/no-kit.txt" "$out/root-kit.txt" \
        || { echo "error: just policy printed more for a passing root deploy kit than for none" >&2; exit 1; }
      echo "// A code change beside the kit." >> "$source"
      just policy rule-changes
      mkdir "$crate_kit"
      printf '#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p "$HOME/.local/share/demo"\n' > "$crate_kit/install.sh"
      printf '[Unit]\nDescription=demo\n' > "$crate_kit/demo.service"
      just policy rule-changes
      for kit in deploy "$crate_kit"; do
        printf '#!/bin/sh\nrm -rf $1/old\n' > "$kit/backup"
        if just policy > "$out/policy.txt" 2>&1 || ! grep --quiet "$kit/backup: shellcheck reports" "$out/policy.txt"; then
          cat "$out/policy.txt" >&2
          echo "error: just policy passed a script in $kit that shellcheck faults" >&2
          exit 1
        fi
        rm "$kit/backup"
      done
      git checkout --quiet -- crates && rm -r deploy "$crate_kit"
      if [ "{{ kind }}" != library ]; then
        # `cargo add --path` gives the dependency on the other crate a `version`, which
        # release-plz would rewrite beside the root's at each release, so `just policy` fails.
        core=$(basename crates/*_core)
        cargo remove --quiet --offline --package "${core%_core}" "$core"
        cargo add --quiet --offline --package "${core%_core}" --path "crates/$core"
        if just policy > "$out/policy.txt" 2>&1 || ! grep --quiet "\[internal_dependency_versions\] crates/${core%_core}/Cargo.toml: \`\[dependencies\] $core\` gives" "$out/policy.txt"; then
          cat "$out/policy.txt" >&2
          echo "error: just policy passed a version on the path dependency cargo add wrote" >&2
          exit 1
        fi
        git checkout --quiet -- crates Cargo.lock
        # Without `changelog_update = false` in `[workspace]`, release-plz would write the other
        # crate's changelog into its directory, which is code, so `just policy` fails.
        sed '/^changelog_update = false$/d' release-plz.toml > "$out/release-plz.toml"
        cp "$out/release-plz.toml" release-plz.toml
        if just policy > "$out/policy.txt" 2>&1 || ! grep --quiet "\[release_changelog\] release-plz.toml: release-plz could write the changelog of \`$core\` to \`crates/$core/CHANGELOG.md\`" "$out/policy.txt"; then
          cat "$out/policy.txt" >&2
          echo "error: just policy passed a release-plz.toml that writes a crate's changelog into the crate" >&2
          exit 1
        fi
        git checkout --quiet -- release-plz.toml
      fi
      # A crate's own `version`, which release-plz would rewrite beside the root's, fails
      # `just policy`.
      manifest="${source%%/src/*}/Cargo.toml"
      sed 's/^version\.workspace = true$/version = "0.1.0"/' "$manifest" > "$out/Cargo.toml"
      cp "$out/Cargo.toml" "$manifest"
      if just policy > "$out/policy.txt" 2>&1 || ! grep --quiet "\[member_manifests\] $manifest: \`\[package\] version\` is \"0.1.0\"" "$out/policy.txt"; then
        cat "$out/policy.txt" >&2
        echo "error: just policy passed a crate with a version of its own" >&2
        exit 1
      fi
      git checkout --quiet -- crates
      if [ "{{ publish }}" = true ]; then
        # A published crate's package holds only the files under its directory, so without its
        # license file there it would reach crates.io without the license text: `just policy`
        # and `just package` fail.
        crate="${source%%/src/*}"
        rm "$crate/LICENSE"
        if just policy > "$out/policy.txt" 2>&1 || ! grep --quiet "\[license_files\] $crate/Cargo.toml: .* but \`$crate/\` lacks \`LICENSE\`" "$out/policy.txt"; then
          cat "$out/policy.txt" >&2
          echo "error: just policy passed a published crate without its license file" >&2
          exit 1
        fi
        if just package > "$out/package.txt" 2>&1 || ! grep --quiet "the package of $crate lacks LICENSE" "$out/package.txt"; then
          cat "$out/package.txt" >&2
          echo "error: just package passed a published crate without its license file" >&2
          exit 1
        fi
        git checkout --quiet -- crates
      fi
      # Without release-plz.toml, release-plz falls back to its defaults, which write each crate's
      # changelog into the crate, so `just policy` fails.
      mv release-plz.toml "$out/release-plz.toml"
      if just policy > "$out/policy.txt" 2>&1 || ! grep --quiet '\[release_changelog\] release-plz.toml: `release-plz.toml` is missing' "$out/policy.txt"; then
        cat "$out/policy.txt" >&2
        echo "error: just policy passed a project without release-plz.toml" >&2
        exit 1
      fi
      mv "$out/release-plz.toml" release-plz.toml
    fi
    if [ "{{ kind }}" = library ] && [[ " {{ recipes }} " = *" dylint "* || "{{ recipes }}" = ci ]]; then
      # A lint library built from another trellis commit is deleted before `just dylint` runs, as
      # after an update, since cargo-dylint would keep using it.
      echo "an older trellis commit" > target/dylint/trellis-rev
      touch target/dylint/libraries/built-from-the-older-commit
      just dylint
      [ ! -e target/dylint/libraries/built-from-the-older-commit ] \
        || { echo "error: just dylint kept a lint library built from another trellis commit" >&2; exit 1; }
      [ "$(tr -d '\r\n' < target/dylint/trellis-rev)" = "$(just --evaluate trellis_rev | tr -d '\r\n')" ] \
        || { echo "error: just dylint recorded \"$(cat target/dylint/trellis-rev)\" as the trellis commit its lint library came from" >&2; exit 1; }
      # A library built from this commit stays, so CI's cache keeps saving the build.
      touch target/dylint/libraries/built-from-this-commit
      just dylint
      [ -e target/dylint/libraries/built-from-this-commit ] \
        || { echo "error: just dylint deleted a lint library built from this trellis commit" >&2; exit 1; }
    fi
    if [[ "{{ recipes }}" = ci || " {{ recipes }} " = *" notices "* ]] && [ -f about.toml ]; then
      # A rules pull request accepts a license for a crate before the code pull request adds it:
      # `just notices` passes the entry, and `just stale-licenses` fails until the crate arrives.
      cp about.toml "$out/about.toml"
      # The second entry sits on line 99, where cargo-about widens its line numbers.
      printf '\n[not-a-dependency]\naccepted = ["CC0-1.0"]\n' >> about.toml
      while [ "$(wc -l < about.toml)" -lt 98 ]; do echo >> about.toml; done
      printf '[gone-dependency]\naccepted = ["MIT"]\n' >> about.toml
      just notices
      if [ "$(uname -s)" = Linux ]; then
        if just stale-licenses > "$out/stale.txt" 2>&1 || ! grep --quiet 'has 2 entries' "$out/stale.txt" \
          || ! grep --quiet '^error: .*\[not-a-dependency\]' "$out/stale.txt" \
          || ! grep --quiet '^error: .*\[gone-dependency\]' "$out/stale.txt"; then
          cat "$out/stale.txt" >&2
          echo "error: just stale-licenses didn't name every entry for a crate the graph doesn't hold" >&2
          exit 1
        fi
      fi
      cp "$out/about.toml" about.toml
      if [ "$(uname -s)" = Linux ]; then just stale-licenses; fi
      echo "just notices passed an about.toml entry ahead of its crate, and just stale-licenses named it."
    fi
    if [[ "{{ recipes }}" = ci || " {{ recipes }} " = *" notices "* ]] && [ "{{ web }}" = true ]; then
      # A license accepted for one npm package in web-licenses.toml, and in a rules pull request
      # before the code pull request adds the package, as for a crate. The package stands in for a
      # font under a license `accepted` lacks; a frozen install leaves it in place.
      cp web-licenses.toml "$out/web-licenses.toml"
      font="crates/demo_{{ kind }}/web/node_modules/@demo/font"
      mkdir -p "$font"
      printf '{"name": "@demo/font", "version": "1.0.0", "license": "OFL-1.1"}\n' > "$font/package.json"
      if just notices > "$out/web-notices.txt" 2>&1 \
        || ! grep --quiet --extended-regexp '^(error: |::error::)@demo/font 1.0.0 .* has the license `OFL-1.1`, which isn.t accepted' "$out/web-notices.txt"; then
        cat "$out/web-notices.txt" >&2
        echo "error: just notices passed an npm package whose license nothing accepts" >&2
        exit 1
      fi
      entry() { printf '\n[[package]]\nname = "%s"\nlicenses = ["OFL-1.1"]\nreason = "%s"\n' "$1" "$2" >> web-licenses.toml; }
      entry @demo/font "A font the web UI ships."
      just notices
      grep --quiet --line-regexp --fixed-strings '@demo/font 1.0.0: OFL-1.1' target/THIRD-PARTY-LICENSES-WEB.txt \
        || { echo "error: the web notices don't list the package web-licenses.toml accepts" >&2; exit 1; }
      entry @demo/gone "A font the web UI dropped."
      just notices
      if [ "$(uname -s)" = Linux ]; then
        if just stale-licenses > "$out/stale.txt" 2>&1 \
          || ! grep --quiet --extended-regexp '^(error: |::error::)web-licenses.toml accepts licenses for @demo/gone, which isn.t installed' "$out/stale.txt" \
          || grep --quiet 'for @demo/font,' "$out/stale.txt"; then
          cat "$out/stale.txt" >&2
          echo "error: just stale-licenses didn't name only the web-licenses.toml entry for a package not installed" >&2
          exit 1
        fi
      fi
      rm -r "$font"
      cp "$out/web-licenses.toml" web-licenses.toml
      just web-notices
      if [ "$(uname -s)" = Linux ]; then just stale-licenses; fi
      echo "just notices passed a web-licenses.toml entry for its package alone and ahead of it, and just stale-licenses named it."
    fi
    if [ "{{ recipes }}" = ci ] && [ "{{ kind }}" = binary ] && [ "$(uname -s)" = Linux ]; then
      # The release build's own steps, run for this machine's target, must archive the program, its
      # license files and notices, and a web UI's build and notices only when it has one.
      "$trellis/tests/release-build/run.sh" "$project" > "$out/dist.txt"
      [ "$(wc -l < "$out/dist.txt")" -eq 1 ] || { echo "error: the release build left other than one archive:" >&2; cat "$out/dist.txt" >&2; exit 1; }
      tar --list --gzip --file "$(cat "$out/dist.txt")" | sed 's|^[^/]*/||' | sort > "$out/archive.txt"
      expected="demo-binary README.md THIRD-PARTY-LICENSES.html LICENSE"
      if [ "{{ web }}" = true ]; then expected="$expected THIRD-PARTY-LICENSES-WEB.txt web/index.html"; fi
      for file in $expected; do
        grep --quiet --line-regexp --fixed-strings "$file" "$out/archive.txt" \
          || { echo "error: the release archive lacks $file:" >&2; cat "$out/archive.txt" >&2; exit 1; }
      done
      if [ "{{ web }}" != true ] && grep --quiet '^web/' "$out/archive.txt"; then
        echo "error: the release archive of a binary without a web UI has web/" >&2; exit 1
      fi
      if [ "{{ web }}" = true ]; then
        mkdir "$out/archive"
        tar --extract --gzip --file "$(cat "$out/dist.txt")" --directory "$out/archive" --strip-components 1
        grep --quiet 'src="./index-[a-z0-9]*\.js"' "$out/archive/web/index.html" \
          || { echo "error: the release archive's web UI isn't the build" >&2; exit 1; }
        grep --quiet typescript "$out/archive/THIRD-PARTY-LICENSES-WEB.txt" \
          || { echo "error: the release archive's web notices don't list typescript" >&2; exit 1; }
      fi
    fi
    if [ "{{ recipes }}" = ci ] && [ "{{ kind }}" = service ] && command -v docker > /dev/null; then
      # A checkout or a `just notices` under umask 077 leaves the license files 0600, which the
      # image's non-root user can't read unless the image sets their modes.
      chmod 600 LICENSE* target/THIRD-PARTY-LICENSES*
      (umask 077 && just image)
      docker run --rm demo-service:dev --version
      container=$(docker create demo-service:dev)
      trap 'docker rm "$container" > /dev/null; rm -rf "$out"' EXIT
      docker cp "$container:/usr/share/doc/demo-service" - | tar --list --verbose > "$out/docs.txt"
      if grep --invert-match --quiet --extended-regexp '^(drwxr-xr-x|-rw-r--r--) ' "$out/docs.txt" || ! grep --quiet '^-' "$out/docs.txt"; then
        echo "error: the image's license files aren't readable by its user:" >&2
        cat "$out/docs.txt" >&2
        exit 1
      fi
      if [ "{{ web }}" = true ]; then
        docker cp "$container:/usr/share/demo-service/web/index.html" "$out/index.html"
        grep --quiet 'src="./index-[a-z0-9]*\.js"' "$out/index.html" || { echo "error: the image's web UI isn't the build" >&2; exit 1; }
        docker cp "$container:/usr/share/doc/demo-service/THIRD-PARTY-LICENSES-WEB.txt" "$out/notices.txt"
        grep --quiet typescript "$out/notices.txt" || { echo "error: the image's web notices don't list typescript" >&2; exit 1; }
        docker image inspect --format '{{{{ json .Config.Env }}' demo-service:dev \
          | grep --quiet '"DEMO_SERVICE_WEB_DIR=/usr/share/demo-service/web"' \
          || { echo "error: the image doesn't name its web UI's directory in DEMO_SERVICE_WEB_DIR" >&2; exit 1; }
        web=crates/demo_service/web
        # Bun's frozen install passes with no lockfile and picks the versions itself, so a missing
        # bun.lock fails the install, in the recipe and in the image.
        mv "$web/bun.lock" "$out/bun.lock"
        missing="has no bun.lock"
        if just web-install > "$out/web-install.txt" 2>&1 || ! grep --quiet "^error: .*$missing" "$out/web-install.txt"; then
          echo "error: web-install ran without a bun.lock" >&2; exit 1
        fi
        # The plain log also prints the command, which holds the message, so match only its output.
        if docker build --progress plain --target web --file Containerfile . > "$out/web-image.txt" 2>&1 \
          || ! grep --quiet --extended-regexp "^#[0-9]+ [0-9.]+ error: .*$missing" "$out/web-image.txt"; then
          echo "error: the image's install ran without a bun.lock" >&2; exit 1
        fi
        mv "$out/bun.lock" "$web/bun.lock"
        # A package that provides a `bun` command would run the web UI's scripts on that Bun, so
        # the install fails, in the recipe and in the image. The image's install reads the app's
        # bunfig.toml, which here leaves that package out, so the same lockfile then builds.
        (cd "$web" && bun add --dev "bun@$(bun --version)") > "$out/bun-add.txt" 2>&1 \
          || { cat "$out/bun-add.txt" >&2; exit 1; }
        refused="holds a bun or bunx command"
        if just web-install > "$out/web-install.txt" 2>&1 || ! grep --quiet "^error: .*$refused" "$out/web-install.txt"; then
          echo "error: web-install took a package that provides a bun command" >&2; exit 1
        fi
        if docker build --progress plain --target web --file Containerfile . > "$out/web-image.txt" 2>&1 \
          || ! grep --quiet --extended-regexp "^#[0-9]+ [0-9.]+ error: .*$refused" "$out/web-image.txt"; then
          echo "error: the image's install took a package that provides a bun command" >&2; exit 1
        fi
        printf '[install]\ndev = false\n' > "$web/bunfig.toml"
        # Bun keeps a package it installed earlier, whatever the settings say now.
        rm -rf "$web/node_modules"
        just web-install
        docker build --target web --file Containerfile . > "$out/web-image.txt" 2>&1 \
          || { echo "error: the image's install didn't read the web UI's bunfig.toml" >&2; cat "$out/web-image.txt" >&2; exit 1; }
      fi
    fi

# Generates a binary whose `build` answer sets every key, and checks that CI's build job and the
# release's run .github/setup-build.sh between the toolchain and the first build, with the
# timeout, build for Linux with glibc and install no musl-gcc, that the release build runs on a
# change to the script, that the starter script does nothing on every runner, that a failing one
# fails with its output, that `just actionlint` shellchecks it, and, on a glibc Linux host, that
# the glibc build links dynamically. Its docs must say the release needs glibc. The answer's
# checks must refuse what it doesn't take, and another kind's answer, and a binary with the
# default answer must get none of it.
#
# Checks what a binary's `build` answer changes in its builds.
build-extras:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    fail() { echo "error: build-extras: $1" >&2; exit 1; }
    # Generates a binary for Linux in $1, with the further arguments.
    generate() {
      local destination=$1
      shift
      uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
        --data project_name=demo-binary \
        --data "description=A demo binary that trellis generates for its tests." \
        --data kind=binary --data 'platforms=[linux, macos]' --data 'architectures=[x86_64]' \
        --data trellis_path="$trellis" "$@" "$trellis" "$destination"
    }
    while IFS='|' read -r answer message; do
      if refusal=$(generate "$out/refused" --data "build=$answer" 2>&1); then
        fail "the build answer took $answer"
      fi
      grep --quiet --fixed-strings -- "$message" <<< "$refusal" || fail "the build answer refused $answer for another reason: $refusal"
    done << 'EOF'
    [gnu]|Write a mapping
    {libc: glibc}|`libc` is `musl`, the default, or `gnu`
    {setup: "yes"}|`setup` is `true` or `false`
    {timeout: 30}|`timeout` is a number of minutes from 60 to 360
    {timeout: "90"}|`timeout` is a number of minutes from 60 to 360
    {image: x}|`image` is not a key it takes
    EOF
    generate "$out/default"
    if grep --quiet --recursive --extended-regexp 'setup-build|linux-gnu|timeout-minutes: 90' "$out/default/.github"; then
      fail "a binary with the default build answer got build extras"
    fi
    project="$out/demo-binary"
    generate "$project" --data 'build={libc: gnu, setup: true, timeout: 90}'
    cd "$project"
    git init --quiet --initial-branch main
    for workflow in ci build-release; do
      file=".github/workflows/$workflow.yml"
      job=$(awk '/^  build:$/ { inside = 1; next } /^  [a-z-]+:$/ { inside = 0 } inside' "$file")
      grep --quiet '^    timeout-minutes: 90$' <<< "$job" || fail "$file's build job doesn't take 90 minutes"
      grep --quiet 'target: x86_64-unknown-linux-gnu$' <<< "$job" || fail "$file doesn't build for Linux with glibc"
      ! grep --quiet --extended-regexp "musl-(gcc|tools)|linux-musl" <<< "$job" || fail "$file still builds for or installs musl"
      line() { grep --line-number --max-count 1 --fixed-strings -- "$1" <<< "$job" | cut -d: -f1; }
      setup=$(line 'run: bash .github/setup-build.sh')
      toolchain=$(line 'rustup target add "$TARGET"')
      first_build=$(line "$([ "$workflow" = ci ] && echo 'run: just lint' || echo 'run: just notices')")
      [ -n "$setup" ] || fail "$file's build job doesn't run .github/setup-build.sh"
      [ -n "$toolchain" ] && [ -n "$first_build" ] || fail "$file's build job has no toolchain step or first build"
      [ "$setup" -lt "$first_build" ] || fail "$file doesn't run the setup before the first build"
      # The setup frees disk space too, so it runs before any step that restores a cache, which
      # can fill the runner's disk, and in the release, which caches nothing, after the toolchain.
      cache=$(grep --line-number --max-count 1 --extended-regexp 'uses: (Swatinem/rust-cache|actions/cache)' <<< "$job" | cut -d: -f1) || true
      if [ "$workflow" = ci ]; then
        [ -n "$cache" ] && [ "$setup" -lt "$cache" ] || fail "$file restores a cache before it runs the setup"
      else
        [ -z "$cache" ] || fail "$file's build job restores a cache"
        [ "$toolchain" -lt "$setup" ] || fail "$file doesn't run the setup after the toolchain"
      fi
    done
    grep --quiet '^      - .github/setup-build.sh$' .github/workflows/build-release.yml \
      || fail "a pull request that changes the setup script doesn't run the release build"
    grep --quiet "at least as new as Ubuntu 24.04's" docs/install.md || fail "docs/install.md doesn't say the release needs glibc"
    # The starter does nothing, on every runner, and passes; a failing script fails its step, with
    # its own output.
    [ -f .github/setup-build.sh ] || fail "the project has no starter .github/setup-build.sh"
    for os in Linux macOS Windows; do
      RUNNER_OS=$os TARGET=x86_64-unknown-linux-gnu bash .github/setup-build.sh || fail "the starter failed on $os"
    done
    just actionlint zizmor
    printf '#!/usr/bin/env bash\necho "error: no libgtk-3-dev for $TARGET" >&2\nexit 1\n' > .github/setup-build.sh
    if RUNNER_OS=Linux TARGET=x86_64-unknown-linux-gnu bash .github/setup-build.sh 2> "$out/setup.txt" \
      || ! grep --quiet 'no libgtk-3-dev' "$out/setup.txt"; then
      fail "a failing setup script passed or hid its output"
    fi
    printf '#!/usr/bin/env bash\necho Setting up $TARGET\n' > .github/setup-build.sh
    if just actionlint > "$out/actionlint.txt" 2>&1 || ! grep --quiet SC2086 "$out/actionlint.txt"; then
      cat "$out/actionlint.txt" >&2
      fail "just actionlint passed a setup script that shellcheck faults"
    fi
    host=$(rustc -vV | sed -n 's/^host: //p')
    if [[ $host == *-linux-gnu ]]; then
      CARGO_BUILD_TARGET=$host just build-release
      file "target/$host/release/demo-binary" | grep --quiet 'dynamically linked' \
        || fail "the glibc build isn't dynamically linked: $(file "target/$host/release/demo-binary")"
    fi
    # A `build` answer for another kind is refused, since that kind's files would ignore it.
    if refusal=$(uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD --data project_name=demo-service \
      --data "description=A demo service." --data kind=service --data 'build={setup: true}' "$trellis" "$out/service" 2>&1); then
      fail "a service took a build answer"
    fi
    grep --quiet 'Only a binary takes a `build` answer' <<< "$refusal" || fail "a service's build answer was refused for another reason: $refusal"

# Generates a service whose `image` answer sets every key, with files in crates/<crate>/image, and
# builds its image under umask 077 with those files 0600 or 0700. The image must carry each
# environment default as written, the working directory and the ports, keep distroless's nonroot
# user and its own /tmp and /home/nonroot, give that user read access to the files and execute
# access to none, and keep trellis's license file over one the files hold at its path. Without
# the files, or with a file in a directory the base image has, the build must fail and say why.
# A working directory without files must be root's. The answer's checks must refuse the values
# that would break or change the Containerfile, and a service with the default answer must get
# none of it.
#
# Checks what a service's `image` answer adds to its image.
image-extras:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    fail() { echo "error: image-extras: $1" >&2; exit 1; }
    # Generates a service in $1, with the further arguments.
    generate() {
      local destination=$1
      shift
      uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
        --data project_name=demo-service \
        --data "description=A demo service that trellis generates for its tests." \
        --data kind=service --data trellis_path="$trellis" "$@" "$trellis" "$destination"
    }
    # Runs `docker build` in the current directory, which must fail with a line of its own output
    # that matches $1; $2 says what a success did.
    refused_build() {
      if docker build --progress plain --file Containerfile . > "$out/build.txt" 2>&1 \
        || ! grep --quiet --extended-regexp "^#[0-9]+ [0-9.]+ error: .*$1" "$out/build.txt"; then
        cat "$out/build.txt" >&2
        fail "$2"
      fi
    }
    while IFS='|' read -r answer message; do
      if refusal=$(generate "$out/refused" --data "image=$answer" 2>&1); then
        fail "the image answer took $answer"
      fi
      grep --quiet --fixed-strings -- "$message" <<< "$refusal" || fail "the image answer refused $answer for another reason: $refusal"
    done << 'EOF'
    {env: {PORT: 8080}}|needs a quoted one-line string
    {env: {A: "x\ny"}}|needs a quoted one-line string
    {env: {A: "x\u0085y"}}|needs a quoted one-line string
    {env: {"A\n": "x"}}|is not a variable name
    {env: {DEMO_SERVICE_WEB_DIR: /x}}|the variable trellis sets to the web UI
    {workdir: "/app\n"}|`workdir` is an absolute path
    {workdir: /app/../tmp}|`workdir` is an absolute path
    {workdir: /home/app}|`workdir` is under the image's own /home
    {workdir: //tmp}|`workdir` is an absolute path
    {workdir: /./tmp}|`workdir` is an absolute path
    {workdir: /usr/lib/app}|`workdir` is under the image's own /usr
    {expose: ["8080\n"]}|in `expose` is not a port
    {expose: [65536]}|in `expose` is not a port
    {files: "yes"}|`files` is `true` or `false`
    {user: app}|`user` is not a key it takes
    EOF
    if refusal=$(uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD --data project_name=demo-binary \
      --data "description=A demo binary." --data kind=binary --data 'image={files: true}' "$trellis" "$out/binary" 2>&1); then
      fail "a binary took an image answer"
    fi
    grep --quiet 'Only a service takes an `image` answer' <<< "$refusal" || fail "a binary's image answer was refused for another reason: $refusal"
    # Copier stores a mapping answer with its keys sorted, so a render from `--data` and a later one
    # from the stored answers must write the image's environment in one order.
    generate "$out/typed" --data 'image={env: {ZED: "1", http_proxy: "2", HTTP_PROXY: "3", ALPHA: "4"}}'
    generate "$out/stored" --data-file "$out/typed/.copier-answers.yml"
    diff -r "$out/typed" "$out/stored" || fail "a render from the stored image answer differs from the first"
    grep --quiet '^ENV ALPHA="4"' "$out/typed/Containerfile" || fail "the image's environment isn't in sorted order"
    rm -rf "$out/typed" "$out/stored"
    generate "$out/default"
    # A public service's release pushes the image by its digest alone, tries attesting it three
    # times, and tags it with its version, minor and latest only after it is signed and verified,
    # so a failed run leaves no tag on an unsigned image.
    image_job=$(awk '/^  image:$/ { inside = 1; next } /^  [a-z-]+:$/ { inside = 0 } inside' "$out/default/.github/workflows/release.yml")
    grep --quiet --extended-regexp 'outputs: type=image,name=\$\{\{ env\.IMAGE \}\},push-by-digest=true' <<< "$image_job" \
      || fail "the release doesn't push the image by its digest alone"
    ! grep --quiet --extended-regexp '^ +tags:' <<< "$image_job" || fail "the release pushes the image with a tag before it is signed"
    [ "$(grep --count 'uses: actions/attest-build-provenance' <<< "$image_job")" -eq 3 ] \
      || fail "the release doesn't try attesting the image three times"
    step() { grep --line-number --max-count 1 --fixed-strings -- "$1" <<< "$image_job" | cut -d: -f1; }
    attested=$(step 'name: Attest the image a last time')
    signed=$(step 'name: Sign the image and verify')
    tagged=$(step 'name: Tag the image with its version')
    [ -n "$attested" ] && [ -n "$signed" ] && [ -n "$tagged" ] && [ "$attested" -lt "$signed" ] && [ "$signed" -lt "$tagged" ] \
      || fail "the release tags the image before it attests, signs and verifies it"
    tag_step=$(awk '/name: Tag the image with its version/ { inside = 1; print; next } inside && /^      - / { exit } inside' <<< "$image_job")
    grep --quiet --fixed-strings -- '--tag "$IMAGE:$TAG"' <<< "$tag_step" || fail "the release doesn't tag the image with its version in its last tagging step"
    grep --quiet '^    timeout-minutes: 60$' <<< "$image_job" || fail "a public service's image job doesn't take 60 minutes for the attest and sign retries"
    # A private service's release neither attests nor signs, so its image job keeps the limit
    # without the retries' waits, and a hung job stops before it spends more of the account's
    # Actions usage.
    generate "$out/private" --data visibility=private
    image_job=$(awk '/^  image:$/ { inside = 1; next } /^  [a-z-]+:$/ { inside = 0 } inside' "$out/private/.github/workflows/release.yml")
    grep --quiet '^    timeout-minutes: 45$' <<< "$image_job" || fail "a private service's image job doesn't stop at 45 minutes"
    rm -rf "$out/private"
    final_stage=$(awk '/^FROM / { stage = "" } { stage = stage $0 "\n" } END { printf "%s", stage }' "$out/default/Containerfile")
    if grep --quiet --extended-regexp '^(WORKDIR|EXPOSE|ENV) |/files/' <<< "$final_stage"; then
      fail "a service with the default image answer got image extras: $final_stage"
    fi
    if ! command -v docker > /dev/null; then
      echo "image-extras checked the answer, but builds no image without Docker."
      exit 0
    fi
    generate "$out/workdir" --data 'image={workdir: /srv/app}'
    (cd "$out/workdir" && git init --quiet --initial-branch main && just notices && docker build --quiet --file Containerfile --tag demo-service:workdir . > /dev/null)
    listing=$(docker run --rm --entrypoint /usr/local/bin/demo-service demo-service:workdir --version && docker image inspect --format '{{{{ .Config.WorkingDir }}' demo-service:workdir)
    grep --quiet --line-regexp /srv/app <<< "$listing" || fail "the image's working directory isn't /srv/app: $listing"
    container=$(docker create demo-service:workdir)
    docker cp "$container:/srv" - | tar --list --verbose > "$out/workdir.txt"
    docker rm "$container" > /dev/null
    grep --quiet --extended-regexp '^drwxr-xr-x (root/root|0/0) .* srv/app/$' "$out/workdir.txt" \
      || fail "the working directory isn't root's and 0755: $(cat "$out/workdir.txt")"
    project="$out/demo-service"
    generate "$project" --data 'image={env: {HOST: 0.0.0.0, PORT: "8740", QUOTED: "a \"b\" $HOME \\ c"}, workdir: /app, expose: [8740, 9000/udp], files: true}'
    cd "$project"
    git init --quiet --initial-branch main
    files=crates/demo_service/image
    just notices
    refused_build "crates/demo_service/image doesn't exist" "the image built without crates/demo_service/image"
    mkdir -p "$files/tmp"
    echo "scratch" > "$files/tmp/scratch"
    refused_build "crates/demo_service/image/tmp would change the image's own /tmp" "the image's files went into its own /tmp"
    rm -r "$files/tmp"
    mkdir -p "$files/usr/lib"
    echo "not a library" > "$files/usr/lib/libgcc_s.so.1"
    refused_build "usr/lib would change the image's own /usr/lib" "the image's files went into its own /usr/lib"
    rm -r "$files/usr/lib"
    mkdir -p "$files/app"
    cp "$(type -P true)" "$files/app/tool"
    refused_build "app/tool is an ELF program or library" "the image took an ELF file"
    rm "$files/app/tool"
    mkdir -p "$files/app/profiles" "$files/usr/share/doc/demo-service" "$files/app/bin"
    echo '{"name": "default"}' > "$files/app/profiles/default.json"
    echo "A vendored component's notice." > "$files/usr/share/doc/demo-service/NOTICE"
    echo "Not the project's license." > "$files/usr/share/doc/demo-service/LICENSE"
    printf '#!/bin/sh\necho helper\n' > "$files/app/bin/helper"
    chmod 600 "$files/app/profiles/default.json" "$files/usr/share/doc/demo-service/"* LICENSE* target/THIRD-PARTY-LICENSES*
    chmod 700 "$files/app/bin/helper"
    git add --all
    git -c user.name=trellis -c user.email=trellis@localhost commit --quiet --message "chore: generate from trellis"
    just policy
    (umask 077 && just image)
    docker run --rm demo-service:dev --version
    config=$(docker image inspect --format '{{{{ json .Config }}' demo-service:dev)
    for expected in '"HOST=0.0.0.0"' '"PORT=8740"' '"QUOTED=a \"b\" $HOME \\ c"' '"WorkingDir":"/app"' \
      '"8740/tcp":{}' '"9000/udp":{}' '"User":"65532"'; do
      grep --quiet --fixed-strings -- "$expected" <<< "$config" || fail "the image's config lacks $expected: $config"
    done
    container=$(docker create demo-service:dev)
    trap 'docker rm "$container" > /dev/null; rm -rf "$out"' EXIT
    for path in /app/profiles/default.json /usr/share/doc/demo-service/NOTICE /app/bin/helper; do
      docker cp "$container:$path" - | tar --list --verbose > "$out/file.txt"
      grep --quiet --extended-regexp -- "^-rw-r--r-- (root/root|0/0) " "$out/file.txt" || fail "$path isn't -rw-r--r--, owned by root: $(cat "$out/file.txt")"
    done
    docker export "$container" | tar --list --verbose > "$out/image.txt"
    for expected in '^drwxrwxrwt (root/root|0/0) .* tmp/$' '^drwx------ 65532/65532 .* home/nonroot/$'; do
      grep --quiet --extended-regexp -- "$expected" "$out/image.txt" || fail "the image's files changed the base image's directories: $(grep -E ' (tmp|home/nonroot)/$' "$out/image.txt")"
    done
    docker cp "$container:/usr/share/doc/demo-service/LICENSE" "$out/LICENSE"
    cmp --quiet LICENSE "$out/LICENSE" || fail "the image's files replaced trellis's license file"

# Generates a binary whose `release` answer sets every key, with a release/package.sh, and runs its
# release build's own steps for this machine's target, which must leave exactly the files the
# answer names, and fail when the script writes one fewer or one more. The workflows must build
# with the answer's environment, run the release build on a change to the script, and write and
# attest the answer's checksums file under the answer's title; `just actionlint` must shellcheck
# the script, and the docs must list the answer's names. The answer's checks must refuse what it
# doesn't take, and another kind's answer.
#
# Checks what a binary's `release` answer changes in its releases.
release-extras:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    fail() { echo "error: release-extras: $1" >&2; exit 1; }
    triple=x86_64-unknown-linux-musl
    # Generates a binary for Linux on x86-64 in $1, with the further arguments.
    generate() {
      local destination=$1
      shift
      uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
        --data project_name=demo-binary \
        --data "description=A demo binary that trellis generates for its tests." \
        --data kind=binary --data 'platforms=[linux]' --data 'architectures=[x86_64]' \
        --data trellis_path="$trellis" "$@" "$trellis" "$destination"
    }
    while IFS='|' read -r answer message; do
      if refusal=$(generate "$out/refused" --data "release=$answer" 2>&1); then
        fail "the release answer took $answer"
      fi
      grep --quiet --fixed-strings -- "$message" <<< "$refusal" || fail "the release answer refused $answer for another reason: $refusal"
    done << 'EOF'
    {files: {macos: ["demo-{version}.zip"]}}|is not a target triple
    {files: {x86_64-unknown-linux-musl: ["demo+{version}"]}}|that holds `{version}`
    {files: {x86_64-unknown-linux-musl: ["demo.zip"]}}|that holds `{version}`
    {files: {x86_64-unknown-linux-musl: ["demo-{version}", "demo-{version}"]}}|is named twice
    {files: {x86_64-unknown-linux-musl: []}}|lists at least one file name
    {checksums: attestation.json}|other than `attestation.json`
    {name: "Demo\nBinary"}|`name` is up to 64 letters
    {env: {GITHUB_TOKEN: "x"}}|no `GITHUB_`, `RUNNER_` or `CARGO_` prefix
    {env: {RUSTFLAGS: "-C x"}}|none of the variables that change the toolchain
    {env: {target: "x"}}|use capitals
    {env: {DEMO_RELEASE: 1}}|needs a quoted one-line string
    {env: {1: "x", A: "y"}}|is not a variable the release build may set
    {installer: true}|`installer` maps a Windows target triple to its installer
    {files: {x86_64-unknown-linux-musl: ["d-{version}.exe"]}, installer: {x86_64-unknown-linux-musl: {app_id: EBAC94F7-A24E-431F-B4F6-8E9D669E9E53, file: "d-{version}.exe"}}}|is not a Windows target
    {files: {x86_64-pc-windows-msvc: ["d-{version}.exe"]}, installer: {x86_64-pc-windows-msvc: {app_id: "{EBAC94F7-A24E-431F-B4F6-8E9D669E9E53}", file: "d-{version}.exe"}}}|a new GUID without braces
    {files: {x86_64-pc-windows-msvc: ["d-{version}.exe"]}, installer: {x86_64-pc-windows-msvc: {file: "d-{version}.exe"}}}|a new GUID without braces
    {files: {x86_64-pc-windows-msvc: ["d-{version}.exe"]}, installer: {x86_64-pc-windows-msvc: {app_id: EBAC94F7-A24E-431F-B4F6-8E9D669E9E53, file: "e-{version}.exe"}}}|one of the names `files` lists for it
    {files: {x86_64-pc-windows-msvc: ["d-{version}.zip"]}, installer: {x86_64-pc-windows-msvc: {app_id: EBAC94F7-A24E-431F-B4F6-8E9D669E9E53, file: "d-{version}.zip"}}}|ending in `.exe`
    {files: {x86_64-pc-windows-msvc: ["d-{version}.exe"]}, installer: {x86_64-pc-windows-msvc: {app_id: EBAC94F7-A24E-431F-B4F6-8E9D669E9E53, file: "d-{version}.exe", icon: a.ico}}}|`icon` in `installer`'s `x86_64-pc-windows-msvc` is not a key it takes
    {files: {x86_64-pc-windows-msvc: ["d-{version}.exe"]}, installer: {x86_64-pc-windows-msvc: {app_id: EBAC94F7-A24E-431F-B4F6-8E9D669E9E53, file: "d-{version}.exe", name: "A/B"}}}|has a `name` of up to 64 letters
    {files: {x86_64-pc-windows-msvc: ["d-{version}.exe"], aarch64-pc-windows-msvc: ["d-{version}-arm64.exe"]}, installer: {x86_64-pc-windows-msvc: {app_id: EBAC94F7-A24E-431F-B4F6-8E9D669E9E53, file: "d-{version}.exe"}, aarch64-pc-windows-msvc: {app_id: C882D75D-CDF1-4F35-91C7-8D8A7A01A810, file: "d-{version}-arm64.exe"}}}|takes the same `app_id`
    EOF
    if refusal=$(uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD --data project_name=demo-service \
      --data "description=A demo service." --data kind=service --data 'release={checksums: SUMS}' "$trellis" "$out/service" 2>&1); then
      fail "a service took a release answer"
    fi
    grep --quiet 'Only a binary takes a `release` answer' <<< "$refusal" || fail "a service's release answer was refused for another reason: $refusal"
    # Copier stores a mapping answer with its keys sorted, so a render from `--data` and a later one
    # from the stored answers must write the release build's environment in one order.
    generate "$out/typed" --data 'release={env: {ZED: "1", ALPHA: "2"}}'
    generate "$out/stored" --data-file "$out/typed/.copier-answers.yml"
    diff -r "$out/typed" "$out/stored" || fail "a render from the stored release answer differs from the first"
    rm -rf "$out/typed" "$out/stored"
    # With the build answer's glibc, the release answer names the glibc target's files.
    generate "$out/glibc" --data 'build={libc: gnu}' \
      --data 'release={files: {x86_64-unknown-linux-gnu: ["demo-binary-{version}-linux"]}}'
    grep --quiet '^            files: demo-binary-{version}-linux$' "$out/glibc/.github/workflows/build-release.yml" \
      || fail "a glibc release doesn't name the glibc target's files"
    grep --quiet --fixed-strings 'Linux on x86-64: `demo-binary-<version>-linux`' "$out/glibc/docs/install.md" \
      || fail "a glibc release's docs/install.md doesn't list its files"
    # docs/maintaining.md's example release answer names files for exactly the project's targets,
    # so it works as written, and mentions Git Bash only when Windows is a platform.
    # Prints the $2th example release answer in $1's docs/maintaining.md, 1 by default.
    example() {
      awk -v want="${2:-1}" '/--data .release=\{checksums/ { n++ } n == want { print } n == want && /\}\}.$/ { exit }' \
        "$1/docs/maintaining.md" | sed -e "s/^.*--data 'release=//" -e "s/'$//"
    }
    for platforms in '[linux]' '[linux, macos, windows]'; do
      generate "$out/docs" --data "platforms=$platforms" --data 'architectures=[x86_64, aarch64]'
      answer=$(example "$out/docs")
      [ -n "$answer" ] || fail "docs/maintaining.md has no example release answer for $platforms"
      generate "$out/example" --data "platforms=$platforms" --data 'architectures=[x86_64, aarch64]' --data "release=$answer"
      workflow="$out/example/.github/workflows/build-release.yml"
      targets=$(sed -n 's/^            target: //p' "$workflow")
      [ "$(sed -n 's/^            files: .*/x/p' "$workflow" | wc -l)" = "$(wc -l <<< "$targets")" ] \
        || fail "the docs' example release answer for $platforms leaves a target without files: $answer"
      [ "$(grep -o '^ *[a-z0-9_-]*:' <<< "$answer" | tr -d ' :' | sort)" = "$(sort <<< "$targets")" ] \
        || fail "the docs' example release answer for $platforms names other targets than the project's: $answer"
      bullet=$(awk '/^- \*\*Release files\.\*\*/ { on = 1; print; next } on && /^(- \*\*|## )/ { exit } on' "$out/docs/docs/maintaining.md")
      if [[ "$platforms" = *windows* ]]; then
        # The Windows installer's example works as written, given a GUID.
        answer=$(example "$out/docs" 2 | sed 's/<GUID>/EBAC94F7-A24E-431F-B4F6-8E9D669E9E53/g')
        [ -n "$answer" ] || fail "docs/maintaining.md has no example installer answer"
        generate "$out/installer-example" --data "platforms=$platforms" --data 'architectures=[x86_64, aarch64]' --data "release=$answer"
        [ "$(grep --count '^            installer: ' "$out/installer-example/.github/workflows/build-release.yml")" -eq 2 ] \
          || fail "the docs' example installer answer doesn't give both Windows targets an installer: $answer"
        rm -rf "$out/installer-example"
        grep --quiet 'Git Bash on Windows' <<< "$bullet" || fail "the docs don't say the release script runs with Git Bash on Windows"
        grep --quiet 'cygpath -w' <<< "$bullet" || fail "the docs don't say how to call a Windows program from Git Bash"
      elif grep --quiet 'Git Bash\|iscc\|cygpath' <<< "$bullet"; then
        fail "the docs give Windows release advice to a project without Windows: $bullet"
      fi
      rm -rf "$out/docs" "$out/example"
    done
    # A public binary's release tries attesting its archives three times and publishes the bundle of
    # the attempt that worked.
    generate "$out/attested"
    publish_job=$(awk '/^  publish:$/ { inside = 1; next } /^  [a-z-]+:$/ { inside = 0 } inside' "$out/attested/.github/workflows/release.yml")
    [ "$(grep --count 'uses: actions/attest-build-provenance' <<< "$publish_job")" -eq 3 ] \
      || fail "the release doesn't try attesting the archives three times"
    grep --quiet --fixed-strings "steps.attest.outcome == 'success' && steps.attest.outputs.bundle-path || steps.attest-again.outcome == 'success' && steps.attest-again.outputs.bundle-path || steps.attest-last.outputs.bundle-path" <<< "$publish_job" \
      || fail "the release doesn't publish the bundle of the attempt that worked"
    grep --quiet '^    timeout-minutes: 20$' <<< "$publish_job" || fail "a public binary's publish job doesn't take 20 minutes for the attest retries"
    # The update contract docs/stability.md states, which installed copies' updaters rely on: the
    # vX.Y.Z tag, and attestation.json as one bundle whose statement names every release file by
    # its name and SHA-256. The check must fail on each change tests/release/mutate.py makes.
    contract() { uv run --quiet --no-project --with pyyaml==6.0.3 python -I "$trellis/tests/release/contract.py" "$@"; }
    mutate() { uv run --quiet --no-project --with pyyaml==6.0.3 python -I "$trellis/tests/release/mutate.py" "$@"; }
    archive=demo-binary-0.1.0-x86_64-unknown-linux-musl.tar.gz
    contract "$out/attested" "$archive"
    mutations=$(mutate --list)
    [ -n "$mutations" ] || fail "tests/release/mutate.py lists no mutations"
    while IFS= read -r mutation; do
      rm -rf "$out/mutated"
      cp -R "$out/attested" "$out/mutated"
      mutate "$out/mutated" "$mutation"
      if contract "$out/mutated" "$archive" 2> "$out/contract.txt"; then
        fail "the update contract check passed this mutation: $mutation"
      fi
      echo "the update contract check fails as it should on: $mutation"
    done <<< "$mutations"
    rm -rf "$out/mutated"
    rm -rf "$out/attested"
    # A private binary's release attests nothing, so its publish job keeps the limit without the
    # retries' waits, and a hung job stops before it spends more of the account's Actions usage.
    generate "$out/private" --data visibility=private
    publish_job=$(awk '/^  publish:$/ { inside = 1; next } /^  [a-z-]+:$/ { inside = 0 } inside' "$out/private/.github/workflows/release.yml")
    grep --quiet '^    timeout-minutes: 15$' <<< "$publish_job" || fail "a private binary's publish job doesn't stop at 15 minutes"
    rm -rf "$out/private"
    project="$out/demo-binary"
    generate "$project" --data 'release={name: Demo Binary, checksums: SHA256SUMS.txt, files: {x86_64-unknown-linux-musl: ["demo-binary-{version}-linux.tar.gz", "demo-binary-{version}-linux"]}, env: {DEMO_RELEASE: "${{{{ inputs.ref && '"'1'"' || '"''"' }}", QUOTED: "a \"b\" \\ c"}}'
    cd "$project"
    git init --quiet --initial-branch main
    workflow=.github/workflows/build-release.yml
    grep --quiet "^            files: demo-binary-{version}-linux.tar.gz demo-binary-{version}-linux$" "$workflow" \
      || fail "$workflow's matrix doesn't name the target's files"
    grep --quiet '^      - release/\*\*$' "$workflow" || fail "a change to release/ doesn't run the release build"
    grep --quiet --fixed-strings "DEMO_RELEASE: \"\${{{{ inputs.ref && '1' || '' }}\"" "$workflow" \
      || fail "the release build doesn't get DEMO_RELEASE"
    grep --quiet --fixed-strings 'QUOTED: "a \"b\" \\ c"' "$workflow" || fail "the release build's QUOTED isn't escaped"
    ! grep --quiet '^      - name: Archive$' "$workflow" || fail "the release build still archives as trellis does"
    grep --quiet 'run: sha256sum -- \* > SHA256SUMS.txt$' .github/workflows/release.yml || fail "the release doesn't write SHA256SUMS.txt"
    grep --quiet 'subject-checksums: dist/SHA256SUMS.txt$' .github/workflows/release.yml || fail "the release doesn't attest SHA256SUMS.txt"
    grep --quiet -- '--title "Demo Binary ${TAG#v}"' .github/workflows/release.yml || fail "the release's title doesn't start with Demo Binary"
    grep --quiet --fixed-strings 'Linux on x86-64: `demo-binary-<version>-linux.tar.gz`, `demo-binary-<version>-linux`' docs/install.md \
      || fail "docs/install.md doesn't list the release's files"
    grep --quiet 'sha256sum --check --ignore-missing SHA256SUMS.txt' docs/verifying-releases.md \
      || fail "docs/verifying-releases.md doesn't name SHA256SUMS.txt"
    grep --quiet '`SHA256SUMS.txt` and `attestation.json` files' docs/stability.md || fail "docs/stability.md doesn't name SHA256SUMS.txt"
    mkdir release
    cat > release/package.sh << 'EOF'
    #!/usr/bin/env bash
    # Writes the release files for $TARGET to $DIST.
    set -euo pipefail
    staging="$DIST/../staging"
    rm -rf "$staging"
    mkdir "$staging"
    cp "$BINARY" "$NOTICES" "$staging/"
    tar -C "$staging" -czf "$DIST/demo-binary-$VERSION-linux.tar.gz" .
    rm -r "$staging"
    cp "$BINARY" "$DIST/demo-binary-$VERSION-linux"
    if [ -n "${EXTRA:-}" ]; then echo extra > "$DIST/$EXTRA"; fi
    if [ -n "${SKIP:-}" ]; then rm "$DIST/demo-binary-$VERSION-$SKIP"; fi
    if [ -n "${SIDE:-}" ]; then mkdir -p dist && echo side > "dist/$SIDE"; fi
    EOF
    just actionlint zizmor
    export FILES="demo-binary-{version}-linux.tar.gz demo-binary-{version}-linux"
    "$trellis/tests/release-build/run.sh" "$project" > "$out/dist.txt"
    expected=$(printf 'dist/demo-binary-0.1.0-linux\ndist/demo-binary-0.1.0-linux.tar.gz\n')
    [ "$(cat "$out/dist.txt")" = "$expected" ] || fail "the release build left other files than the answer names: $(cat "$out/dist.txt")"
    for wrong in 'EXTRA=notes.txt|must write exactly the files' 'SKIP=linux|must write exactly the files' \
      'SIDE=notes.txt|wrote dist/' 'FILES=|names no files for'; do
      if env "${wrong%%|*}" "$trellis/tests/release-build/run.sh" "$project" > "$out/wrong.txt" 2>&1 \
        || ! grep --quiet --fixed-strings "${wrong#*|}" "$out/wrong.txt"; then
        cat "$out/wrong.txt" >&2
        fail "the release build took what release/package.sh wrote with ${wrong%%|*}"
      fi
    done
    printf '#!/usr/bin/env bash\ncp $BINARY "$DIST"\n' > release/package.sh
    if just actionlint > "$out/actionlint.txt" 2>&1 || ! grep --quiet SC2086 "$out/actionlint.txt"; then
      cat "$out/actionlint.txt" >&2
      fail "just actionlint passed a release/package.sh that shellcheck faults"
    fi
    contract . demo-binary-0.1.0-linux.tar.gz demo-binary-0.1.0-linux
    [ ! -e release/installer.iss ] && [ ! -e release/installer-custom.iss ] \
      || fail "a release answer without an installer got installer files"
    # An installer answer gets release/installer.iss and release/installer-custom.iss, and the
    # Windows target's build installs the pinned Inno Setup and builds the installer. An update
    # rewrites installer.iss, keeps the owner's custom file, and refuses a new app_id. Copier's
    # updates need a clean commit of the template, so it renders from a copy.
    mkdir "$out/trellis"
    (cd "$trellis" && tar --create --exclude=./.git --exclude=./target --exclude=./lints/target --file=- .) \
      | tar --extract --directory="$out/trellis" --file=-
    commit() { git -c user.name=trellis -c user.email=trellis@localhost "$@"; }
    (cd "$out/trellis" && git init --quiet --initial-branch main && git add --all && commit commit --quiet --message v1)
    installer_answer() {
      echo "release={files: {x86_64-pc-windows-msvc: [\"demo-binary-{version}-setup.exe\", \"demo-binary-{version}.zip\"]}, installer: {x86_64-pc-windows-msvc: {app_id: $1, file: \"demo-binary-{version}-setup.exe\"$2}}}"
    }
    app_id=EBAC94F7-A24E-431F-B4F6-8E9D669E9E53
    uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD --data project_name=demo-binary \
      --data "description=A demo binary that trellis generates for its tests." --data kind=binary \
      --data 'platforms=[linux, windows]' --data 'architectures=[x86_64]' \
      --data "$(installer_answer "$app_id" "")" "$out/trellis" "$out/installer" > "$out/copy.txt" 2>&1 \
      || { cat "$out/copy.txt" >&2; fail "a binary with an installer answer didn't generate"; }
    ! grep --quiet Warning "$out/copy.txt" || fail "a new project with an installer warned: $(cat "$out/copy.txt")"
    cd "$out/installer"
    workflow=.github/workflows/build-release.yml
    [ "$(grep --count '^            installer: demo-binary-{version}-setup.exe$' "$workflow")" -eq 1 ] \
      || fail "the Windows target's build doesn't name its installer, or another target's does"
    grep --quiet '^      - name: Install Inno Setup$' "$workflow" || fail "the release build doesn't install Inno Setup"
    grep --quiet 'innosetup-7.1.1-x64.exe$' "$workflow" || fail "the release build doesn't install the pinned Inno Setup"
    grep --quiet 'export STAGING=' "$workflow" || fail "release/package.sh gets no STAGING"
    grep --quiet --fixed-strings "#define AppId \"$app_id\"" release/installer.iss || fail "release/installer.iss doesn't hold the app_id"
    grep --quiet --fixed-strings '#define OutputName "demo-binary-" + AppVersion + "-setup"' release/installer.iss \
      || fail "release/installer.iss doesn't name the installer as the answer's file"
    grep --quiet --line-regexp 'AppPublisher=265866' release/installer.iss || fail "release/installer.iss doesn't name the author as its publisher"
    grep --quiet --line-regexp '#include "installer-custom.iss"' release/installer.iss || fail "release/installer.iss doesn't include the custom file"
    # The include splices the custom file in as text, so a line other than a preprocessor directive
    # between it and [Setup] lands in the custom file's last section, such as [Code].
    between=$(awk '/^#include "installer-custom.iss"$/ { inside = 1; next } /^\[Setup\]$/ { exit } inside && !/^ *(#|$)/' release/installer.iss)
    [ -z "$between" ] || fail "release/installer.iss writes more than directives between its include and [Setup]: $between"
    contract . demo-binary-0.1.0-setup.exe demo-binary-0.1.0.zip
    printf '#!/usr/bin/env bash\nset -euo pipefail\ncp "$BINARY" "$STAGING/$NAME.exe"\n' > release/package.sh
    git init --quiet --initial-branch main
    git add --all
    just actionlint zizmor > /dev/null 2>&1 || fail "the installer's release build fails actionlint or zizmor"
    echo "; The owner's line." >> release/installer-custom.iss
    git add --all
    commit commit --quiet --message "chore: generate from trellis"
    if refusal=$(uv tool run copier@9.19.1 update --quiet --defaults --vcs-ref=:current: \
      --data "$(installer_answer C882D75D-CDF1-4F35-91C7-8D8A7A01A810 "")" 2>&1); then
      fail "an update took a new app_id"
    fi
    grep --quiet --fixed-strings "has \`app_id\` $app_id in .copier-answers.yml" <<< "$refusal" \
      || fail "an update refused a new app_id for another reason: $refusal"
    [ -z "$(git status --porcelain)" ] || fail "the refused update changed files"
    uv tool run copier@9.19.1 update --quiet --defaults --vcs-ref=:current: \
      --data "$(installer_answer "$app_id" ", name: Demo App")" > "$out/update.txt" 2>&1 \
      || { cat "$out/update.txt" >&2; fail "an update with the same app_id and a new name failed"; }
    ! grep --quiet Warning "$out/update.txt" || fail "an installer update warned: $(cat "$out/update.txt")"
    grep --quiet --fixed-strings '#define AppName "Demo App"' release/installer.iss || fail "the update didn't rewrite release/installer.iss"
    grep --quiet --line-regexp "; The owner's line." release/installer-custom.iss || fail "the update dropped the owner's line in release/installer-custom.iss"
    git add --all
    commit commit --quiet --message "chore: rename the installer"
    uv tool run copier@9.19.1 update --quiet --defaults --vcs-ref=:current: > "$out/update.txt" 2>&1 \
      || { cat "$out/update.txt" >&2; fail "an update without answers failed"; }
    ! grep --quiet Warning "$out/update.txt" || fail "an installer update warned: $(cat "$out/update.txt")"
    [ -z "$(git status --porcelain)" ] || fail "an update without answers changed files: $(git status --porcelain)"
    # A new app_id typed at the update's prompt is refused as a passed one is.
    typed=$(installer_answer C882D75D-CDF1-4F35-91C7-8D8A7A01A810 ", name: Demo App")
    outcome=$(uv run --quiet --no-project --with pexpect==4.9.0 python -I "$trellis/tests/release/interactive-update.py" \
      "${typed#release=}") || fail "an interactive update $outcome a new app_id"
    [ -z "$(git status --porcelain)" ] || fail "the refused interactive update changed files"
    # An update that passes another answer keeps the app_id and reads the answers file silently.
    uv tool run copier@9.19.1 update --quiet --defaults --vcs-ref=:current: \
      --data 'description=Another demo binary that trellis generates for its tests.' > "$out/update.txt" 2>&1 \
      || { cat "$out/update.txt" >&2; fail "an update passing another answer failed"; }
    ! grep --quiet Warning "$out/update.txt" || fail "an installer update warned: $(cat "$out/update.txt")"
    grep --quiet --fixed-strings "#define AppId \"$app_id\"" release/installer.iss || fail "an update passing another answer changed the app_id"
    git add --all
    commit commit --quiet --message "docs: describe it again"
    # An update to a later trellis renders this one's answers with its checks, silently too.
    (cd "$out/trellis" && commit commit --quiet --allow-empty --message v2)
    uv tool run copier@9.19.1 update --quiet --defaults --vcs-ref HEAD > "$out/update.txt" 2>&1 \
      || { cat "$out/update.txt" >&2; fail "an update to a later trellis failed"; }
    ! grep --quiet Warning "$out/update.txt" || fail "an update to a later trellis warned: $(cat "$out/update.txt")"
    git add --all
    commit commit --quiet --message "chore: update trellis"
    # An update that drops the installer deletes both files, the owner's custom file too.
    uv tool run copier@9.19.1 update --quiet --defaults --vcs-ref=:current: \
      --data 'release={files: {x86_64-pc-windows-msvc: ["demo-binary-{version}-setup.exe", "demo-binary-{version}.zip"]}}' \
      > "$out/update.txt" 2>&1 || { cat "$out/update.txt" >&2; fail "an update dropping the installer failed"; }
    [ ! -e release/installer.iss ] && [ ! -e release/installer-custom.iss ] \
      || fail "an update dropping the installer left its files"

# Generates a library as a user gets it, with its checker fetched from a scratch copy of this
# checkout in place of GitHub, and checks that `just install-policy` installs the pinned checker
# whatever an earlier install left. The copy has two commits: the pinned one is this working tree,
# and the one before has a checker that says it is stale. Installed one after the other through
# trellis 0.17.3's recipe with one shared target directory, cargo installs the stale build, records
# the pinned commit as installed, and that recipe then keeps the stale checker. With
# CARGO_TARGET_DIR and CARGO_BUILD_BUILD_DIR still naming that directory, `just policy` must install
# the pinned checker without writing there, and pass. Then `just install-policy` must print nothing
# and run no cargo, and must start an install when the binary is missing, has changed at the same
# size, or was recorded at another commit. A failed install must leave neither the record nor its build directory.
#
# Checks that installing the checker recovers from a stale build and a stale record.
checker-install:
    #!/usr/bin/env bash
    set -euo pipefail
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    # Git Bash on Windows: a `C:/...` path means the same directory to bash, git, Copier and Cargo.
    if command -v cygpath > /dev/null; then
      out=$(cygpath --mixed "$out")
    fi
    fail() { echo "error: checker-install: $1" >&2; exit 1; }
    commit() { git -c user.name=trellis -c user.email=trellis@localhost "$@"; }
    mkdir "$out/trellis"
    tar --create --exclude=./.git --exclude=./target --exclude=./lints/target --file=- . \
      | tar --extract --directory="$out/trellis" --file=-
    cd "$out/trellis"
    git init --quiet --initial-branch main
    cp policy/src/main.rs "$out/main.rs"
    line='    let holds = format!("All {} structure rules hold in {root}", Rule::ALL.len());'
    awk -v old="$line" '!done && $0 == old { sub(/"All/, "\"The stale checker: all"); done = 1 } { print } END { exit !done }' \
      "$out/main.rs" > policy/src/main.rs || fail "policy/src/main.rs has no line $line"
    git add --all
    commit commit --quiet --message stale
    stale=$(git rev-parse HEAD)
    cp "$out/main.rs" policy/src/main.rs
    commit commit --quiet --all --message pinned
    pinned=$(git rev-parse HEAD)
    export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0="url.$out/trellis.insteadOf" \
      GIT_CONFIG_VALUE_0=https://github.com/265866/trellis CARGO_NET_GIT_FETCH_WITH_CLI=true
    uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD --data project_name=demo-checker \
      --data "description=A demo library that trellis generates for its tests." --data kind=library \
      "$out/trellis" "$out/demo"
    cd "$out/demo"
    git init --quiet --initial-branch main
    git add --all
    commit commit --quiet --message "chore: generate from trellis"
    [ "$(just --evaluate trellis_rev | tr -d '\r\n')" = "$pinned" ] || fail "the project doesn't pin the scratch commit"
    old() {
      cargo install --quiet --locked --root target/trellis-policy --git https://github.com/265866/trellis \
        --rev "$1" trellis-policy
    }
    stale_checker() { grep --quiet 'The stale checker' <<< "$(target/trellis-policy/bin/trellis-policy check)"; }
    CARGO_TARGET_DIR="$out/shared" old "$stale"
    CARGO_TARGET_DIR="$out/shared" old "$pinned"
    stale_checker || fail "trellis 0.17.3's recipe installed the pinned checker, so this test no longer builds the state it repairs"
    old "$pinned"
    stale_checker || fail "trellis 0.17.3's recipe left the stale checker without the shared target directory, so this test no longer builds the state it repairs"
    touch "$out/before"
    CARGO_TARGET_DIR="$out/shared" CARGO_BUILD_BUILD_DIR="$out/shared" just policy > "$out/policy.txt" 2> "$out/policy.err" \
      || { cat "$out/policy.txt" "$out/policy.err" >&2; fail "just policy failed over a stale checker and its record"; }
    written=$(find "$out/shared" -newer "$out/before")
    [ -z "$written" ] || fail "the install wrote to the target directory CARGO_TARGET_DIR names: $written"
    if grep --quiet 'The stale checker' "$out/policy.txt" || ! grep --quiet 'structure rules hold' "$out/policy.txt"; then
      cat "$out/policy.txt" >&2
      fail "just policy ran a checker other than the pinned one"
    fi
    tr -d '\r' < "$out/policy.err" | grep --quiet --line-regexp --fixed-strings "Installing trellis-policy at $pinned." \
      || { cat "$out/policy.err" >&2; fail "just policy didn't say it installed the checker"; }
    # With RUSTC naming no file, cargo fails as soon as it starts, so a run that succeeds ran none.
    install() { RUSTC="$out/no-rustc" just install-policy > "$out/install.txt" 2>&1; }
    install || { cat "$out/install.txt" >&2; fail "just install-policy started an install over the pinned checker"; }
    [ ! -s "$out/install.txt" ] || { cat "$out/install.txt" >&2; fail "just install-policy printed something when it had nothing to install"; }
    bin=target/trellis-policy/bin/trellis-policy
    if [ -f "$bin.exe" ]; then bin=$bin.exe; fi
    stamp=target/trellis-policy/trellis-rev
    cp "$bin" "$out/checker"
    cp "$stamp" "$out/stamp"
    reinstalls() {
      if install; then fail "just install-policy kept the checker when $1"; fi
      grep --quiet --fixed-strings "Installing trellis-policy at $pinned." "$out/install.txt" \
        || { cat "$out/install.txt" >&2; fail "just install-policy failed without starting an install when $1"; }
      [ ! -e "$stamp" ] || fail "a failed install left $stamp"
      [ ! -e target/trellis-policy/build ] || fail "a failed install left target/trellis-policy/build"
      cp "$out/checker" "$bin"
      cp "$out/stamp" "$stamp"
    }
    rm "$bin"
    reinstalls "its binary was missing"
    byte=$(dd if="$bin" bs=1 skip=4096 count=1 2> /dev/null | od -An -tu1 | tr -d ' ')
    printf "\\$(printf '%03o' $(( byte ^ 1 )))" | dd of="$bin" bs=1 seek=4096 conv=notrunc 2> /dev/null
    reinstalls "its binary had changed at the same size"
    awk '{ $1 = "an-older-commit"; print }' "$out/stamp" > "$stamp"
    reinstalls "it was recorded at another commit"
    install || { cat "$out/install.txt" >&2; fail "just install-policy started an install over the restored checker"; }
    echo "just install-policy replaced a stale checker and its record, and skipped only the pinned one."

# A generated service's CI builds the image for every architecture on an x86-64 runner, as its
# release does, cross-compiling for the others. CI's `Service image` jobs build the arm64 one here
# and run it on an ARM64 runner. `archive` is an absolute path.
#
# Generates a service and writes its image for one platform, such as `linux/arm64`, to `archive`.
service-image platform archive:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    project="$out/demo-service"
    uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
      --data project_name=demo-service \
      --data "description=A demo service that trellis generates for its tests." \
      --data kind=service \
      --data trellis_path="$trellis" \
      "$trellis" "$project"
    cd "$project"
    just notices
    docker buildx build --file Containerfile --platform "{{ platform }}" --tag demo-service:ci \
      --output "type=docker,dest={{ archive }}" .

# tests/dependencies/setup.sh adds the dependencies most projects use (serde, serde_json,
# thiserror, anyhow, clap, tokio, tracing and futures) with code that uses them in trellis's
# idioms, and the checks that read that code run. Then tests/dependencies/violations.sh adds, one
# at a time, code that breaks a rule next to them, and checks that the lints reject each.
#
# Generates a binary with common dependencies and checks what the lints accept and reject.
dependencies:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    project="$out/demo-binary"
    uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
      --data project_name=demo-binary \
      --data "description=A demo binary that trellis generates for its tests." \
      --data kind=binary \
      --data trellis_path="$trellis" \
      "$trellis" "$project"
    "$trellis/tests/dependencies/setup.sh" "$project"
    cd "$project"
    git init --quiet --initial-branch main
    for recipe in fmt-check typos lint dylint test shear deny notices; do
      just "$recipe"
    done
    cargo run --locked --quiet --manifest-path "$trellis/Cargo.toml" --package trellis-policy -- check
    "$trellis/tests/dependencies/violations.sh" "$project"

# Checks the real shape of each answer set in `shape`.
shapes: (shape "binary") (shape "binary-windows-macos-dual") (shape "library-published") (shape "library-unlicensed") (shape "service-unlicensed") (shape "binary-private") (shape "service-private") (shape "library-private")

# Generates the project a user gets for `answers`, with the lints and the checker from GitHub rather
# than this checkout, and runs the checks that don't fetch them: formatting, spelling, the
# workflows, dependencies, links, the license notices, and this checkout's `trellis-policy check`. A
# published library also runs its `package` recipe, which no other recipe here runs on Unix. The
# copy runs without `--quiet`, so Copier prints the next steps, which must name the repository and
# install just. `just setup-github --dry-run` must print the required checks and the label, and the
# Actions policy and vulnerability reporting only for a public repository, without calling `gh`.
# `just ci` must compare rule changes with `origin/main`, and `just ci <base>` with that base, and
# `just --list` must describe `ci`.
# `just check-github` runs against the fixtures in tests/check-github/, and
# `just check-release-token` and `just set-release-token` against the fake gh in
# tests/check-release-token/. The private binary and library get a 55-character name, the longest
# Copier takes and the longest for which a binary's scaffold is formatted as generated, so a line
# that fits only a short name fails `fmt-check`, and a name one character longer must be refused.
# Each MIT license file must name 265866, the default author, as the only copyright holder, and an
# unlicensed project must have no license file.
#
# Checks the project a user gets for one set of answers.
shape answers:
    #!/usr/bin/env bash
    set -euo pipefail
    name=demo-shape
    case "{{ answers }}" in
      binary) data=(--data kind=binary) ;;
      binary-windows-macos-dual) data=(--data kind=binary --data 'platforms=[windows, macos]' --data 'license=MIT OR Apache-2.0') ;;
      library-published) data=(--data kind=library --data publish_crate=true --data 'license=MIT OR Apache-2.0') ;;
      library-unlicensed) data=(--data kind=library --data license=none) ;;
      service-unlicensed) data=(--data kind=service --data license=none) ;;
      binary-private) data=(--data kind=binary --data visibility=private) name=demo-binary-with-the-longest-name-the-template-supports ;;
      service-private) data=(--data kind=service --data visibility=private) ;;
      library-private) data=(--data kind=library --data visibility=private) name=demo-library-with-the-longest-name-the-template-formats ;;
      *) echo "error: no answer set named {{ answers }}" >&2; exit 2 ;;
    esac
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    project="$out/$name"
    fail() { echo "error: {{ answers }}: $1" >&2; exit 1; }
    has() { grep --quiet --fixed-strings -- "$2" <<< "$1"; }
    copied=$(uv tool run copier@9.19.1 copy --defaults --vcs-ref HEAD \
      --data "project_name=$name" \
      --data "description=A demo that trellis generates for its tests." \
      "${data[@]}" "$trellis" "$project" 2>&1) || { echo "$copied" >&2; exit 1; }
    # With the default author, each MIT license file names 265866, and only 265866, as the copyright
    # holder, and an unlicensed project has no license file.
    licenses=$(find "$project" -name .git -prune -o -name 'LICENSE*' -type f -print)
    case "{{ answers }}" in
      *-unlicensed) [ -z "$licenses" ] || fail "an unlicensed project has license files: $licenses" ;;
      *) [ -n "$licenses" ] || fail "the project has no license file" ;;
    esac
    while read -r license; do
      case "$(basename "$license")" in
        LICENSE | LICENSE-MIT)
          holders=$(grep '^Copyright (c)' "$license") || fail "${license#"$project/"} names no copyright holder"
          [ "$holders" = 'Copyright (c) 265866' ] || fail "${license#"$project/"} names another copyright holder: $holders"
          ;;
      esac
    done <<< "$licenses"
    if [ "${#name}" -eq 55 ]; then
      longer=$(uv tool run copier@9.19.1 copy --defaults --vcs-ref HEAD \
        --data "project_name=${name}x" \
        --data "description=A demo that trellis generates for its tests." \
        "${data[@]}" "$trellis" "$out/longer" 2>&1) && fail "Copier took a name of 56 characters"
      has "$longer" 'Use at most 55 characters.' || fail "Copier refused a longer name for another reason: $longer"
    fi
    has "$copied" "Follow \"Before the first push\" in docs/maintaining.md. It creates 265866/$name" \
      || fail "Copier didn't print the next steps"
    has "$copied" 'cargo binstall just@' || fail "Copier's next steps don't install just"
    cd "$project"
    # A binary's Linux archives use musl. A public binary or service builds for ARM64 by default, and
    # a private one doesn't, since ARM64 runners draw on its account's Actions usage.
    case "{{ answers }}" in
      binary)
        for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-pc-windows-msvc; do
          grep --quiet "target: $target\$" .github/workflows/build-release.yml \
            || fail "the release build lacks $target"
        done
        ;;
      binary-private)
        ! grep --quiet aarch64-unknown-linux .github/workflows/ci.yml \
          || fail "a private binary builds for ARM64 by default"
        ;;
      service-unlicensed)
        grep --quiet '^          platforms: linux/amd64,linux/arm64$' .github/workflows/release.yml \
          || fail "the release doesn't build the image for amd64 and arm64"
        ;;
      service-private)
        grep --quiet '^          platforms: linux/amd64$' .github/workflows/release.yml \
          || fail "a private service's release doesn't build the image for amd64 only"
        ! grep --quiet --extended-regexp '\b[a-z]+-[0-9.]+-arm\b' .github/workflows/ci.yml \
          || fail "a private service runs on ARM64 by default"
        ;;
    esac
    git init --quiet --initial-branch main
    for recipe in fmt-check typos actionlint zizmor shear deny links; do
      just "$recipe"
    done
    mkdir "$out/bin"
    printf '#!/bin/sh\necho "error: the dry run called gh" >&2\nexit 1\n' > "$out/bin/gh"
    chmod +x "$out/bin/gh"
    dry_run=$(PATH="$out/bin:$PATH" just setup-github --dry-run)
    for expected in '"context": "PR title", "integration_id": 15368' '"context": "CI passed", "integration_id": 15368' \
      '"context": "Rule changes stand alone", "integration_id": 15368' '"allow_merge_commit": false' \
      "POST repos/265866/$name/labels" '"name": "release"' "POST repos/265866/$name/rulesets"; do
      has "$dry_run" "$expected" || fail "the setup-github dry run lacks $expected"
    done
    public=$(grep --quiet '^visibility: public$' .copier-answers.yml && echo yes || echo no)
    for public_only in "repos/265866/$name/actions/policies" "PUT repos/265866/$name/private-vulnerability-reporting"; do
      if has "$dry_run" "$public_only"; then
        [ "$public" = yes ] || fail "the setup-github dry run of a private repository has $public_only"
      else
        [ "$public" = no ] || fail "the setup-github dry run of a public repository lacks $public_only"
      fi
    done
    for base in "" origin/stacked; do
      ci_plan=$(just --dry-run ci $base 2>&1) || fail "just ci ${base:-without a base} doesn't run: $ci_plan"
      grep --quiet -- "trellis-policy rule-changes ${base:-origin/main}$" <<< "$ci_plan" \
        || fail "just ci ${base:-without a base} doesn't run rule-changes against ${base:-origin/main}"
    done
    just --list | grep --quiet '^ *ci .*# Runs every check CI runs\.$' || fail "just --list doesn't describe ci"
    "$trellis/tests/check-github/test.sh" "$project"
    "$trellis/tests/check-release-token/test.sh" "$project"
    if [ -f about.toml ]; then
      just notices
    fi
    if [ "{{ answers }}" = library-published ]; then
      just package
    fi
    cargo run --locked --quiet --manifest-path "$trellis/Cargo.toml" --package trellis-policy -- check

# Generates a project for each primary interface, for a web app with a command-line tool and for a
# service with one, and checks its README outline with `tests/outlines/check.py`: a prompt in each
# part the owner writes, shaped by the interfaces in their order, no image, and no line of prose
# over 100 characters. A web, desktop or terminal interface must get `docs/usage.md` with its
# screenshots prompts, CONTRIBUTING.md's Screenshots rules and the pull request template's line
# about images, and any other none of them. Each outline, untouched, must pass
# `trellis-policy check` and `just links`.
#
# Checks the README outline each interface gets.
outlines:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    # A library is asked neither interface question. service,cli puts the primary interface after
    # its other one in the order of the choices, which Copier stores a multiselect answer in.
    for spec in binary:web:web_ui=true binary:desktop binary:terminal binary:cli service:service \
      library:library binary:web,cli:web_ui=true service:web:web_ui=true service:service,cli; do
      IFS=: read -r kind interfaces web <<< "$spec"
      project="$out/$kind-${interfaces/,/-}"
      answers=()
      if [ "$kind" != library ]; then
        others=""
        [[ $interfaces != *,* ]] || others=${interfaces#*,}
        answers=(--data "interface=${interfaces%%,*}" --data "other_interfaces=[$others]")
      fi
      uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
        --data project_name=demo --data "description=A demo that trellis generates." \
        --data kind="$kind" "${answers[@]}" ${web:+--data "$web"} \
        --data trellis_path="$trellis" "$trellis" "$project"
      uv run --quiet --no-project python -I tests/outlines/check.py "$project" "$interfaces"
      (cd "$project" && just links)
      cargo run --locked --quiet --manifest-path "$trellis/Cargo.toml" --package trellis-policy -- \
        check --manifest-path "$project/Cargo.toml"
    done
    echo "Each interface's README outline holds its prompts and no image, and passes the docs checks."

# Generates a binary, a service, a library and a published library, and checks with
# `tests/release-gate/gate.py` that the release pull request's CI and the release workflow both run
# `just policy-release`, the second before anything is tagged or published, so a release can't
# publish while a README prompt is left. gate.py must fail on each change
# `tests/release-gate/mutate.py` makes.
#
# Checks that the release routes run the release check of the docs.
release-gate:
    #!/usr/bin/env bash
    set -euo pipefail
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    fail() { echo "error: release-gate: $1" >&2; exit 1; }
    python() { uv run --quiet --no-project --with pyyaml==6.0.3 python -I "$@"; }
    for answers in kind=binary kind=service kind=library "kind=library publish_crate=true license=MIT"; do
      project="$out/demo"
      rm -rf "$project"
      data=()
      for answer in $answers; do data+=(--data "$answer"); done
      uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD --data project_name=demo \
        --data "description=A demo that trellis generates." "${data[@]}" \
        --data trellis_path="$trellis" "$trellis" "$project"
      python "$trellis/tests/release-gate/gate.py" "$project" || fail "the release gate is open for $answers"
      mutations=$(python "$trellis/tests/release-gate/mutate.py" --list "$project")
      [ -n "$mutations" ] || fail "tests/release-gate/mutate.py lists no mutations for $answers"
      while IFS= read -r mutation; do
        rm -rf "$out/mutated"
        cp -R "$project" "$out/mutated"
        python "$trellis/tests/release-gate/mutate.py" "$out/mutated" "$mutation"
        if python "$trellis/tests/release-gate/gate.py" "$out/mutated" 2> "$out/gate.txt"; then
          fail "gate.py passed this mutation for $answers: $mutation"
        fi
        echo "gate.py fails as it should for $answers on: $mutation"
      done <<< "$mutations"
    done
