# Contributing to trellis

## Set up

Install [rustup](https://rustup.rs/), [cargo-binstall](https://github.com/cargo-bins/cargo-binstall),
[uv](https://docs.astral.sh/uv/), [Go](https://go.dev/dl/) for actionlint,
[shellcheck](https://www.shellcheck.net/), Docker, [Bun](https://bun.com/) for the web UI
template, and just (`cargo binstall just@1.59.0`). Then install the check tools with the commands
of the `install-tools` recipe in [template/justfile.jinja](template/justfile.jinja), cargo-about
included, and run `just install-nightly-rustfmt`.

## Check a change

`just ci` runs the checks CI runs, except the ones that need GitHub or another platform: `PR title`,
and the template entries CI runs on macOS and Windows, `binary on macOS`, `binary on Windows` and
`published library on Windows`. Besides the checks a generated project has, it runs:

| Recipe | Checks |
| --- | --- |
| `just pins` | The template pins the same toolchains as this repository and `lints/`. The config files, workflows and `[workspace.lints]` tables both keep agree, apart from what only some answers get. The versions `template/dylint.toml.jinja` says trellis read each trusted macro crate at are the ones `tests/dependencies/setup.sh` pins. Every tool has one version wherever it's pinned. |
| `just lints-lint`, `just lints-comments`, `just lints-test` | clippy, `code_comment` and the UI tests for the custom lints. The lints' own code has no code comments either; the UI fixtures in `lints/ui/` keep theirs, since their test directives are comments. |
| `just template <kind> [recipes] [publish] [web]` | Generates a `binary`, `service` or `library` project from the working tree and runs its `just ci`, or the recipes you name. `true` as `publish` makes a library a published one, and `true` as `web` gives a service a web UI, whose build and notices must reach its image. |
| `just shapes` | Generates the project a user gets, with the lints from GitHub, for each set of answers the `shape` recipe lists, and runs the checks that don't fetch them. It also checks the next steps Copier prints, what `just setup-github --dry-run` would set, and what `just check-github` reports for the fixtures in `tests/check-github/`. |
| `just dependencies` | Generates a binary that uses the crates most projects use, as `tests/dependencies/setup.sh` adds them, runs its checks, then checks that the lints reject each way past them in `tests/dependencies/violations.sh`. |
| `just update` | Generates a project from one scratch trellis version, edits it as an owner would, updates it to a second, and checks the rules moved, the owner's code didn't, and the owner's lines in trellis's files merged without conflicts. Then it switches the project to private and back, and publishes its crate under each license, which must deliver that license's files to the crate. |
| `just templates` | `template` for each kind, `shapes`, `dependencies` and `update`. |

A generated project's check that fails is a trellis bug: fix the template, the lints or the
checker, never the generated project.

[docs/style.md](docs/style.md) is the generated projects' style guide, not trellis's own: it names
checks such as `just policy` that only a project has. It is a copy of `template/docs/style.md`, and
`just pins` keeps the two equal.

## Change a rule

A rule change reaches every project on its next update, and its update PR can't also change code. So
a new lint or check must pass on the template's own files and on `just dependencies`. Add the way
past it that it closes to `violations.sh`, and say what it enforces in `template/docs/style.md` and
its copy `docs/style.md`. Nothing may get looser than the Google Rust Style Guide asks; a relaxation
needs a false positive on code that follows it, shown in a test.

## Add a lint

1. Write it in `lints/src/<lint>.rs`. Its `declare_lint!` doc block says what it checks, the
   Google guide rule it enforces, what it leaves alone, and an example with its fix.
2. Register it: `mod <lint>;` and its pass in `PASSES`, both in `lints/src/lib.rs`.
3. Add its UI test: fixtures in `lints/ui/<lint>/`, with the cases it flags and the ones it
   leaves alone, and a `#[test]` in its file that calls `crate::ui_harness::ui_test`.
4. Write the expected output. Run `just lints-test`: the new test fails and prints "Actual stderr
   saved to" a file for each fixture. Read it, and copy it to the fixture's name with `.stderr`
   for `.rs`, such as `lints/ui/<lint>/<lint>.stderr`, once every report in it is one you meant.
   The `--bless` flag the output suggests doesn't work here.
5. Add its row to the "Checked by tools" table in `template/docs/style.md` and `docs/style.md`.
6. If it takes no exception, add it to `NO_EXCEPTION_LINTS` in `lints/src/suppression_policy.rs`
   and to the list of lints without exceptions under "Silencing a lint" in both `style.md` files.
7. If it takes a setting, add a field for it to `Config` in `lints/src/config.rs`, whose doc
   comment names the lint, and read it with `crate::config::get`. Every key is required and has no
   default, so set it in `dylint.toml`, which `just dylint` and the UI tests read, and in
   `template/dylint.toml.jinja`, with a comment that says what it allows. A UI test of another
   value passes `crate::ui_harness::dylint_toml_with` to its test's `dylint_toml`.
8. Do what [Change a rule](#change-a-rule) asks.

## Update pinned versions

Renovate bumps the actions, the container base images and the stable toolchain, in this repository
and in `template/` together. These move by hand:

- **The lints' nightly**, with each stable Rust release, so the lints never build on an older
  compiler than the projects. Run `cargo dylint upgrade lints`, which moves
  `lints/rust-toolchain.toml` and `clippy_utils` together, fix what the newer compiler internals
  break, and set `lints_nightly` in `template/justfile.jinja` to match. A newer compiler can word
  or place a report differently; rewrite the `.stderr` files it changes as step 4 of
  [Add a lint](#add-a-lint) says.
- **The rustfmt nightly**: `nightly` in `justfile`, `template/justfile.jinja` and the Editors
  section of `template/CONTRIBUTING.md.jinja`.
- **cargo-dylint and dylint-link**, at the version of `dylint_linting` in `lints/Cargo.toml`.
- **The check tools**, on the workflows' `tool:` lines and in `install-tools` in
  `template/justfile.jinja`; just and shellcheck in the Set up section of
  `template/CONTRIBUTING.md.jinja`, and just also in this file's Set up section, `README.md`,
  `copier.yml` and `template/docs/maintaining.md.jinja`; release-plz's `version:` inputs and the
  `tool@x.y.z` mentions in the comments of `template/.github/workflows/`; actionlint's `go install`
  in `template/justfile.jinja` and the workflows; and copier's version in `justfile`.
- **Bun**, with its image in `template/Containerfile.jinja`, which Renovate moves: `bun-version:` in
  the workflows and in `template/.github/workflows/`, and the Set up section of
  `template/CONTRIBUTING.md.jinja`. Then regenerate the web UI starter's lockfile,
  `bun.lock.jinja`, with that Bun, as its packages also move by hand: install the starter's
  `package.json` in a scratch directory and copy the lockfile back, with the project's name
  templated.
- **`rust-version`** in both `Cargo.toml` files: push it to Renovate's toolchain pull request,
  which fails `just pins` until then.
- **The crates `just dependencies` adds**, in `tests/dependencies/setup.sh`. For each crate that
  `trusted_macro_crates` in `template/dylint.toml.jinja` lists, read what its macros write at the
  new version, as that file's comments describe, and update the comment's version and its
  description. A crate whose macros now write more lint levels may have to leave the list.

`just pins` fails until every copy of a version agrees.

## Set up the repository

Install the [Renovate app](https://github.com/apps/renovate), allow squash merging only, and
protect `main`, requiring `PR title` and `CI passed`. `CI passed` passes only when every other job
in CI passes, so the required checks stay the same when CI gains or loses a job. The `Advisories`
workflow runs `just deny` every week, so a new advisory against the root or `lints/` lockfile shows
on `main` before a pull request trips over it.

## Release trellis

Projects update to trellis's newest `vX.Y.Z` tag. Before 1.0, a release that asks something of
projects bumps the minor version: a new or stricter rule, a setting to change, or a step to do by
hand. Any other release bumps the patch version.

To release:

1. Add the release's section to [CHANGELOG.md](CHANGELOG.md), headed with its tag, in a pull
   request. List what changed, and under "Action required in projects", what an owner must do
   before or after the update merges.
2. Once it merges and CI passes on `main`, tag that commit with an annotated tag and push the tag.
   Renovate opens the update in every project.

## Send a change

Pull requests are squash-merged, so the title becomes the commit on `main` and must be a
[conventional commit](https://www.conventionalcommits.org/), such as
`feat(lints): flag wildcard re-exports`.
