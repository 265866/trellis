# Every check CI runs is a recipe here, so `just ci` before pushing runs the same checks.

# Only the formatter runs on this nightly. The policy crate builds on the stable toolchain in
# rust-toolchain.toml, and the lints on the nightly in lints/rust-toolchain.toml.
nightly := "nightly-2026-10-28"

# rustdoc warnings fail `just docs` and the doctests.
export RUSTDOCFLAGS := "-D warnings"

# clippy reads only the root `clippy.toml`, whatever other clippy configuration a directory holds.
export CLIPPY_CONF_DIR := justfile_directory()

# Lists the recipes.
default:
    @just --list

# Runs every check CI runs.
ci: fmt-check typos actionlint zizmor shear deny links pins lint dylint test docs lints-lint lints-comments lints-test templates

# Installs the pinned nightly rustfmt that `just fmt` uses.
install-nightly-rustfmt:
    rustup toolchain install {{ nightly }} --profile minimal --component rustfmt

# Formats the Rust and TOML files.
fmt:
    cargo +{{ nightly }} fmt --all
    cargo +{{ nightly }} fmt --all --manifest-path lints/Cargo.toml
    taplo fmt

# Checks the Rust and TOML files are formatted.
fmt-check:
    cargo +{{ nightly }} fmt --all -- --check
    cargo +{{ nightly }} fmt --all --manifest-path lints/Cargo.toml -- --check
    taplo fmt --check

# Checks spelling in code and docs.
typos:
    typos

# Checks the GitHub workflows, and their shell scripts if shellcheck is installed.
actionlint:
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

# The template's files link to files that exist only once a project is generated, so the
# template tests check those.
#
# Checks that every relative link and anchor in the Markdown files resolves.
links:
    lychee --offline --include-fragments --no-progress --exclude-path target --exclude-path lints/target --exclude-path template './**/*.md'

# Checks that the template pins the same toolchains as this repository and its lints, that the
# config files both keep are the same, and that every tool has one version wherever it's pinned.
#
# Checks that pinned versions and the shared config files agree.
pins:
    #!/usr/bin/env bash
    set -euo pipefail
    channel() { sed -n 's/^channel = "\(.*\)"$/\1/p' "$1"; }
    variable() { sed -n "s/^$1 := \"\(.*\)\"$/\1/p" "$2"; }
    status=0
    check() {
      if [ "$2" != "$3" ]; then
        echo "error: $1: $2 != $3" >&2
        status=1
      fi
    }
    check "template/rust-toolchain.toml and rust-toolchain.toml" \
      "$(channel template/rust-toolchain.toml)" "$(channel rust-toolchain.toml)"
    check "lints_nightly in template/justfile.jinja and lints/rust-toolchain.toml" \
      "$(variable lints_nightly template/justfile.jinja)" "$(channel lints/rust-toolchain.toml)"
    key() { sed -n "s/^$1 = \"=\{0,1\}\(.*\)\"$/\1/p" "$2"; }
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
    for file in rustfmt.toml clippy.toml .taplo.toml .editorconfig .gitattributes .config/nextest.toml docs/style.md; do
      if ! cmp --quiet "$file" "template/$file"; then
        echo "error: $file differs from template/$file" >&2
        status=1
      fi
    done
    versions=$(grep --only-matching --no-filename --extended-regexp \
      '\b[a-z][a-z0-9-]*@v?[0-9]+\.[0-9]+\.[0-9]+\b' \
      .github/workflows/*.yml template/.github/workflows/* justfile template/justfile.jinja \
      CONTRIBUTING.md template/CONTRIBUTING.md.jinja tests/dependencies/*.sh | sed 's/@v/@/; s/^typos-cli@/typos@/; s/^taplo-cli@/taplo@/'
      sed -n 's/^ *version: \([0-9.]*\)$/release-plz@\1/p' template/.github/workflows/release.yml.jinja
      sed -n 's/.*\[shellcheck\]([^)]*) \([0-9.]*\),.*/shellcheck@\1/p' template/CONTRIBUTING.md.jinja)
    versions=$(sort --unique <<< "$versions")
    for tool in $(cut -d@ -f1 <<< "$versions" | uniq --repeated); do
      echo "error: $tool is pinned to more than one version:" $(grep "^$tool@" <<< "$versions") >&2
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

# Runs `template` for every kind, then `shapes`, `dependencies` and `update`.
templates: (template "binary") (template "service") (template "library") shapes dependencies update

# Copies the working tree into a scratch repository as two tagged trellis versions, generates a
# library from the first, edits its code and changelog the way an owner would, changes the
# template's scaffold, a rule file and a docs page, and runs `copier update` to the second. The
# rule file and docs page must move, and the owner's code and changelog must not. Then it makes the
# project private and public again with `--vcs-ref=:current:`, which must keep v2 and the owner's
# code.
#
# Checks that `copier update` moves the rules and leaves the owner's code alone.
update:
    #!/usr/bin/env bash
    set -euo pipefail
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    commit() { git -c user.name=trellis -c user.email=trellis@localhost "$@"; }
    copier() { uv tool run copier@9.19.1 "$@"; }
    mkdir "$out/trellis"
    tar --create --exclude=./.git --exclude=./target --exclude=./lints/target --file=- . \
      | tar --extract --directory="$out/trellis" --file=-
    cd "$out/trellis"
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
    lib=crates/demo/src
    printf '//! Mine.\n\nmod parse;\n\npub use parse::parse;\n' > "$lib/lib.rs"
    printf '/// Parses.\n#[must_use]\npub fn parse() -> u8 {\n    1\n}\n' > "$lib/parse.rs"
    rm "$lib/greeting.rs"
    echo "- An owner's line." >> CHANGELOG.md
    git add --all
    commit commit --quiet --message "feat: parse"
    cd "$out/trellis"
    scaffold="template/crates/{% if kind == 'library' %}{{{{ crate_name }}{% endif %}/src"
    sed -i.bak 's/Hello, /Hi, /' "$scaffold/greeting.rs" && rm "$scaffold/greeting.rs.bak"
    printf '/// New.\npub fn added() {}\n' > "$scaffold/added.rs"
    echo "# A rule a later trellis adds." >> template/rustfmt.toml
    printf '# Added\n\nA page a later trellis adds.\n' > template/docs/added.md
    git add --all
    commit commit --quiet --message v2
    commit tag --annotate --message v2 v2
    cd "$out/demo"
    copier update --quiet --defaults --vcs-ref v2
    fail() { echo "error: $1" >&2; exit 1; }
    grep --quiet '^_commit: v2$' .copier-answers.yml || fail "the answers don't record v2"
    tail -n 1 rustfmt.toml | grep --quiet 'A rule a later trellis adds' || fail "rustfmt.toml didn't move"
    [ -f docs/added.md ] || fail "the new docs page wasn't added"
    grep --quiet '^//! Mine.$' "$lib/lib.rs" || fail "the owner's lib.rs was rewritten"
    [ ! -e "$lib/greeting.rs" ] || fail "the deleted greeting.rs came back"
    [ ! -e "$lib/added.rs" ] || fail "a new scaffold file was added to the owner's crate"
    tail -n 1 CHANGELOG.md | grep --quiet "An owner's line" || fail "CHANGELOG.md was rewritten"
    if grep --recursive --quiet '^<<<<<<<' --exclude-dir=.git .; then fail "the update left conflict markers"; fi
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
    done
    echo "copier update moved the rules and left the owner's code alone."

# Generates a project of one kind, `binary`, `service` or `library`, from the working tree, with
# the lints and checker from this checkout, and runs `recipes` in it, `ci` by default. `true` as
# `publish` makes a library a published one. After `ci`, a service also builds and runs its image
# when Docker is installed.
#
# Generates a project of one kind and runs its checks.
template kind recipes="ci" publish="false":
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
    uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
      --data project_name=demo-{{ kind }} \
      --data "description=A demo {{ kind }} that trellis generates for its tests." \
      --data kind={{ kind }} \
      --data trellis_path="$trellis" \
      $( [ "{{ publish }}" = true ] && echo "--data publish_crate=true --data license=MIT" ) \
      "$trellis" "$project"
    cd "$project"
    git init --quiet --initial-branch main
    git add --all
    git -c user.name=trellis -c user.email=trellis@localhost commit --quiet --message "chore: generate from trellis"
    git update-ref refs/remotes/origin/main HEAD
    for recipe in {{ recipes }}; do
      just "$recipe"
    done
    if [ "{{ recipes }}" = ci ] && [ "{{ kind }}" = service ] && command -v docker > /dev/null; then
      just image
      docker run --rm demo-service:dev --version
    fi

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
# published library must also package the README and its licenses.
#
# Checks the project a user gets for one set of answers.
shape answers:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ answers }}" in
      binary) data=(--data kind=binary) ;;
      binary-windows-macos-dual) data=(--data kind=binary --data 'platforms=[windows, macos]' --data 'license=MIT OR Apache-2.0') ;;
      library-published) data=(--data kind=library --data publish_crate=true --data 'license=MIT OR Apache-2.0') ;;
      library-unlicensed) data=(--data kind=library --data license=none) ;;
      service-unlicensed) data=(--data kind=service --data license=none) ;;
      binary-private) data=(--data kind=binary --data visibility=private) ;;
      service-private) data=(--data kind=service --data visibility=private) ;;
      library-private) data=(--data kind=library --data visibility=private) ;;
      *) echo "error: no answer set named {{ answers }}" >&2; exit 2 ;;
    esac
    trellis="{{ justfile_directory() }}"
    out=$(mktemp -d)
    trap 'rm -rf "$out"' EXIT
    project="$out/demo-shape"
    uv tool run copier@9.19.1 copy --quiet --defaults --vcs-ref HEAD \
      --data project_name=demo-shape \
      --data "description=A demo that trellis generates for its tests." \
      "${data[@]}" "$trellis" "$project"
    cd "$project"
    git init --quiet --initial-branch main
    for recipe in fmt-check typos actionlint zizmor shear deny links; do
      just "$recipe"
    done
    if [ -f about.toml ]; then
      just notices
    fi
    if [ "{{ answers }}" = library-published ]; then
      files=$(cargo package --list --locked --allow-dirty --package demo_shape)
      for file in README.md LICENSE-MIT LICENSE-APACHE; do
        if ! grep --quiet --line-regexp --fixed-strings "$file" <<< "$files"; then
          echo "error: the published crate doesn't ship $file" >&2
          exit 1
        fi
      done
    fi
    cargo run --locked --quiet --manifest-path "$trellis/Cargo.toml" --package trellis-policy -- check
