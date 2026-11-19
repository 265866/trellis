# Contributing to trellis

## Set up

Install [rustup](https://rustup.rs/), [uv](https://docs.astral.sh/uv/), Docker, and the tools the
generated projects' `CONTRIBUTING.md` lists. Then run `just install-nightly-rustfmt`.

## Check a change

`just ci` runs every check CI runs. Besides the checks a generated project has, it runs:

| Recipe | Checks |
| --- | --- |
| `just pins` | The template pins the same toolchains and config files as this repository and `lints/`, and every tool has one version wherever it's pinned. |
| `just lints-lint`, `just lints-comments`, `just lints-test` | clippy, `code_comment` and the UI tests for the custom lints. The lints' own code has no code comments either; the UI fixtures in `lints/ui/` keep theirs, since their test directives are comments. |
| `just template <kind> [recipes] [publish]` | Generates a `binary`, `service` or `library` project from the working tree and runs its `just ci`, or the recipes you name. `true` as `publish` makes a library a published one. |
| `just shapes` | Generates the project a user gets, with the lints from GitHub, for eight sets of answers, and runs the checks that don't fetch them. |
| `just dependencies` | Generates a binary that uses serde, thiserror, anyhow, clap, tokio and tracing, runs its checks, then checks that the lints reject each way past them in `tests/dependencies/violations.sh`. |
| `just update` | Generates a project from one scratch trellis version, edits it as an owner would, updates it to a second, and checks the rules moved and the owner's code didn't. Then it switches the project to private and back. |
| `just templates` | `template` for each kind, `shapes`, `dependencies` and `update`. |

A generated project's check that fails is a trellis bug: fix the template, the lints or the
checker, never the generated project.

## Change a rule

A rule change reaches every project on its next update, and its update PR can't also change code. So
a new lint or check must pass on the template's own files and on `just dependencies`. Add the way
past it that it closes to `violations.sh`, and say what it enforces in `docs/style.md`. Nothing may
get looser than the Google Rust Style Guide asks; a relaxation needs a false positive on code that
follows it, shown in a test.

## Update pinned versions

Renovate bumps the actions, the container base images and the stable toolchain, in this repository
and in `template/` together. These move by hand:

- **The lints' nightly**, with each stable Rust release, so the lints never build on an older
  compiler than the projects. Run `cargo dylint upgrade lints`, which moves
  `lints/rust-toolchain.toml` and `clippy_utils` together, fix what the newer compiler internals
  break, and set `lints_nightly` in `template/justfile.jinja` to match.
- **The rustfmt nightly**: `nightly` in `justfile`, `template/justfile.jinja` and the Editors
  section of `template/CONTRIBUTING.md.jinja`.
- **cargo-dylint and dylint-link**, at the version of `dylint_linting` in `lints/Cargo.toml`.
- **The check tools**, on the workflows' `tool:` lines, in both `CONTRIBUTING` files and in
  `tests/dependencies/`; the release-plz `version:` inputs in `template/.github/workflows/`; and
  actionlint's `go install` and copier's version in the justfiles and workflows.
- **`rust-version`** in both `Cargo.toml` files: push it to Renovate's toolchain pull request,
  which fails `just pins` until then.

`just pins` fails until every copy of a version agrees.

## Set up the repository

Install the [Renovate app](https://github.com/apps/renovate), allow squash merging only, and
protect `main`, requiring `PR title`, `Quick checks`, `Policy and custom lints`, every `Template`
job, `Template shapes` and `Template with common dependencies`.

## Release trellis

Projects update to trellis's newest `vX.Y.Z` tag. Tag a release from `main` once its CI passes,
with an annotated tag, and Renovate opens the update in every project.

## Send a change

Pull requests are squash-merged, so the title becomes the commit on `main` and must be a
[conventional commit](https://www.conventionalcommits.org/), such as
`feat(lints): flag wildcard re-exports`.
