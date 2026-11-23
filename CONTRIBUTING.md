# Contributing to trellis

## Set up

Install [rustup](https://rustup.rs/), [cargo-binstall](https://github.com/cargo-bins/cargo-binstall),
[uv](https://docs.astral.sh/uv/), [Go](https://go.dev/dl/) for actionlint,
[shellcheck](https://www.shellcheck.net/), [jq](https://jqlang.org/), Docker,
[Bun](https://bun.com/) for the web UI template, and just (`cargo binstall just@1.59.0`). Then install the check tools with the commands
of the `install-tools` recipe in [template/justfile.jinja](template/justfile.jinja), cargo-about
included, and run `just install-nightly-rustfmt`.

## Check a change

`just ci` runs the checks CI runs, except the ones that need GitHub or another target: `PR title`,
and the template entries CI runs for a generated binary's other targets and on Windows,
`binary for Linux on x86-64` (musl), `binary on Linux on ARM64`, `binary on macOS`,
`binary on Windows`, `binary on Windows on ARM64`, `binary with a web UI on Windows`,
`binary with an installer on Windows`, `binary with an installer on Windows on ARM64` and
`published library on Windows`. To run one
of them where its runner's system matches yours, pass its target to `just template`, such as
`just template binary "test build-release" false false x86_64-unknown-linux-musl`. Nor does it
run the `C code for musl` jobs, `Checker install (Windows)`, `Service image for ARM64` or
`Run the service image on ARM64`. The third cross-compiles a generated service's arm64 image, as
`just service-image linux/arm64 <absolute path>` does, and the fourth starts it on ARM64. Besides
the checks a generated project has, it runs:

| Recipe | Checks |
| --- | --- |
| `just pins` | The template pins the same toolchains as this repository and `lints/`. The config files, workflows and `[workspace.lints]` tables both keep agree, apart from what only some answers get. The template's two lists of accepted licenses agree. The versions `template/dylint.toml.jinja` says trellis read each trusted macro crate at are the ones `tests/dependencies/setup.sh` pins. The Web UI guide in `template/docs/maintaining.md.jinja` writes `bun.lock` with the command the `web-install` recipe's error names. Every tool and runner image has one version wherever it's pinned in a form the recipe reads, such as `name@version`, in every file git has or would add, binary ones included, apart from `CHANGELOG.md`, `policy/` and the web starter's lockfile, and a conflict names the file and line of each version. A value it can't find or read fails. |
| `just lints-lint`, `just lints-comments`, `just lints-test` | clippy, `code_comment` and the UI tests for the custom lints. The lints' own code has no code comments either; the UI fixtures in `lints/ui/` keep theirs, since their test directives are comments. |
| `just template <kind> [recipes] [publish] [web] [target] [installer] [interfaces]` | Generates a `binary`, `service` or `library` project from the working tree and runs its `just ci`, or the recipes you name. `true` as `publish` makes a library a published one, `true` as `web` gives a binary or service a web UI, whose build and notices must reach its image or, for a binary, the archive its release build's steps write for this machine's target. A service's image must give its user read access to its license files, even under umask 077. A `target` runs `test` and `build-release` for that target. `true` as `installer`, on a Windows runner with a Windows `target`, gives a binary an installer for it, which `tests/installer/run.sh` builds with the release build's own steps, and with Inno Setup 6 for the version before, then installs, upgrades, upgrades while a program in the install runs with and without a window, and uninstalls. `interfaces`, such as `desktop` or `web,cli`, answers `interface` with the first and `other_interfaces` with the rest, in place of their defaults. After `just ci`, `just policy-release` must fail on the README outline's prompts. |
| `just shapes` | Generates the project a user gets, with the lints from GitHub, for each set of answers the `shape` recipe lists, and runs the checks that don't fetch them. It also checks the next steps Copier prints, what `just setup-github --dry-run` would set, what `just check-github` reports for the fixtures in `tests/check-github/`, and that each license file names the default author, `265866`, as the copyright holder. |
| `just dependencies` | Generates a binary that uses the crates most projects use, as `tests/dependencies/setup.sh` adds them, runs its checks, then checks that the lints reject each way past them in `tests/dependencies/violations.sh`. |
| `just outlines` | Generates a project for each primary interface, a web app with a command-line tool, and a service with a command-line tool, whose primary interface comes after its other in the choices, and checks with `tests/outlines/check.py` that its README outline has a `**To write:**` prompt in each part the owner writes, shaped by the interfaces, and no image. A web, desktop or terminal interface must get `docs/usage.md`, the screenshots rules in `CONTRIBUTING.md` and the pull request template's line about images, and any other none of them. Each outline must pass `trellis-policy check`, with its prompts as notices, and `just links`, and fail `trellis-policy check --release` on its prompts and, for a web, desktop or terminal interface, on its missing picture. Written by `tests/outlines/answer.py` as an owner would, it must pass both. |
| `just update` | Checks that every exclusion in `copier.yml` renders with only the answers trellis 0.15.0 asked, as Copier renders them against the version a project updates from. Generates a project from one scratch trellis version, edits it as an owner would, updates it to a second, and checks the rules moved, the owner's code, README and screenshots page didn't, and the owner's lines in trellis's files merged without conflicts. Then it switches the project to private and back, and publishes its crate under each license, which must deliver that license's files to the crate. |
| `just build-extras` | Generates a binary whose `build` answer builds for glibc, runs `.github/setup-build.sh` and raises the timeout, and checks both build jobs run the script after the toolchain, with the timeout and the glibc target and no musl-gcc, that `just actionlint` shellchecks the script, and that the glibc build links dynamically. A binary with the default answer gets none of it. |
| `just image-extras` | Generates a service whose `image` answer sets environment defaults, a working directory, ports and files, builds its image, and checks the image carries each, keeps its nonroot user and trellis's license file, and lets that user read the files. A service with the default answer gets none of them. |
| `just release-gate` | Generates a binary, a service, a library and a published library, and checks with `tests/release-gate/gate.py` that CI runs `just policy-release` on exactly the pull requests from a `release-plz-` branch, and that the release workflow runs it before release-plz tags or publishes, when the push merged such a pull request, found from GitHub's list of the commit's pull requests or from the ` (#N)` that ends a squash merge's subject when that pull request was merged as the commit, or, where the release job reads a `v*` tag on the commit, when it has one. A library also checks a push without a number that counts, apart from the push that creates `main`; a binary or service checks again after release-plz when it released unchecked, before anything builds or publishes. Neither step may continue on an error, and `release-plz.toml` must keep release-plz's default `pr_branch_prefix` and `release_always = false`, which the condition follows. It runs the workflow's own step that finds the release, in a git repository with a stand-in `gh`. gate.py must fail on each change `tests/release-gate/mutate.py` makes. |
| `just release-extras` | Generates a binary whose `release` answer names its files, checksums file, title and build environment, runs its release build's own steps with a `release/package.sh`, and checks they leave exactly the named files and fail on one fewer or one more, that the workflows and docs carry the answer, and that `just actionlint` shellchecks the script. The answer's checks must refuse what it doesn't take. An `installer` answer must get its Inno Setup script and custom file, and a binary without one neither. An update must keep the custom file, refuse a new `app_id` passed or typed at its prompt, which `tests/release/interactive-update.py` types, read the answers file without a warning, and delete both files when the answer drops the installer. A public binary's release must keep the update contract `docs/stability.md` promises: the `vX.Y.Z` tag, and one attestation bundle that names each release file by its name and SHA-256. `tests/release/contract.py` checks it and must fail on each change `tests/release/mutate.py` makes. |
| `just checker-install` | Generates a library as a user gets it, with its checker fetched from a scratch copy of this checkout, and checks that `just install-policy` installs the pinned checker whatever an earlier install left: a stale build in a target directory that `CARGO_TARGET_DIR` names, which trellis 0.17.3 installed and then kept, and a binary that is missing, changed or recorded at another commit. A run with the pinned checker installed must print nothing. CI runs it on Windows too, where the project installs through PowerShell. |
| `just templates` | `template` for each kind, and for a binary with each UI interface and a published library, `shapes`, `outlines`, `release-gate`, `dependencies`, `update`, `build-extras`, `image-extras`, `release-extras` and `checker-install`. |

A generated project's check that fails is a trellis bug: fix the template, the lints or the
checker, never the generated project.

[template/docs/style.md](template/docs/style.md) is the generated projects' style guide, not
trellis's own: it names checks such as `just policy` that only a project has. Each project gets it
as its `docs/style.md`.

## Change a rule

A rule change reaches every project on its next update, and its update PR can't also change code. So
a new lint or check must pass on the template's own files and on `just dependencies`. Add the way
past it that it closes to `violations.sh`, and say what it enforces in `template/docs/style.md`.
Nothing may get looser than the Google Rust Style Guide asks; a relaxation needs a false positive on
code that follows it, shown in a test.

## Add a lint

1. Write it in `lints/src/<lint>.rs`. Its `declare_lint!` doc block says what it checks, the
   Google guide rule it enforces or that it is trellis policy, what it leaves alone, and an example
   with its fix.
2. Register it: `mod <lint>;` and its pass in `PASSES`, both in `lints/src/lib.rs`.
3. Add its UI test: fixtures in `lints/ui/<lint>/`, with the cases it flags and the ones it
   leaves alone, and a `#[test]` in its file that calls `crate::ui_harness::ui_test`.
4. Write the expected output. Run `just lints-test`: the new test fails and prints "Actual stderr
   saved to" a file for each fixture. Read it, and copy it to the fixture's name with `.stderr`
   for `.rs`, such as `lints/ui/<lint>/<lint>.stderr`, once every report in it is one you meant.
   The `--bless` flag the output suggests doesn't work here.
5. Add its row to the "Checked by tools" table in `template/docs/style.md`.
6. If it takes no exception, add it to `NO_EXCEPTION_LINTS` in `lints/src/suppression_policy.rs`
   and to the style guide's list of lints without exceptions under "Silencing a lint". A lint
   whose reports all come from the crate root takes none, since an `expect` on an item can't reach
   them. A lint that reports at an impl block, a trait or a module itself goes in
   `BLOCK_LINTS` or `MODULE_LINTS` there instead, so its `expect` can go on it.
7. If it takes a setting, add a field for it to `Config` in `lints/src/config.rs`, whose doc
   comment names the lint, and read it with `crate::config::get`. Every key is required and has no
   default, so set it in `dylint.toml`, which `just dylint` and the UI tests read, and in
   `template/dylint.toml.jinja`, with a comment that says what it allows. A UI test of another
   value passes `crate::ui_harness::dylint_toml_with` to its test's `dylint_toml`.
8. Do what [Change a rule](#change-a-rule) asks.

## Update pinned versions

Renovate moves each of these in one pull request, with every copy in this repository and in
`template/`:

- The actions and the container base images. A new digest for any base image, Bun's included,
  waits the seven days only when its registry says when it was published. The Rust and
  distroless images' tags give no such date, so their new digests come at once.
- The stable toolchain, with `rust-version` in both `Cargo.toml` files. A patch release leaves
  `rust-version` alone.
- Bun: its image in `template/Containerfile.jinja`, `bun-version:` in the workflows and in
  `template/.github/workflows/`, and the Set up section of `template/CONTRIBUTING.md.jinja`.
- The runner images, in every workflow, CI's matrix and `targets` in `copier.yml`.
- The crates `just dependencies` adds, in `tests/dependencies/setup.sh`.

Two of these pull requests need a step by hand before they merge, which their descriptions repeat:

- **Bun's**: move the web UI starter's packages, in its `package.json.jinja`, and regenerate its
  lockfile, `bun.lock.jinja`, with the new Bun, since Renovate reads neither. Install the
  starter's `package.json` in a scratch directory with `bun install --save-text-lockfile` and copy
  the lockfile back, with the project's name templated.
- **The crates `just dependencies` adds**: for each crate that `trusted_macro_crates` in
  `template/dylint.toml.jinja` lists, read what its macros write at the new version, as that
  file's comments describe, and update the comment's version and its description. A crate whose
  macros now write more lint levels may have to leave the list. `just pins` fails until each
  comment names the version `tests/dependencies/setup.sh` pins.

Renovate finds the copies through the custom managers in `renovate.json`. When one of its pull
requests fails `just pins` because a copy wasn't moved, fix the manager that should have found it.

These move by hand:

- **The lints' nightly**, so the lints never build on an older compiler than the projects.
  `just pins` needs the lints' `clippy_utils` to be for a newer Rust than stable. Each
  `clippy_utils` is for the Rust two releases after the stable it comes out beside, so one port
  covers that stable and the next. Port as soon as Renovate's Dependency Dashboard lists a newer
  `clippy_utils` under "Pending Approval", about five weeks before stable catches up. Until the
  port lands, the toolchain pull request for the release that catches up fails `just pins`. Don't
  approve `clippy_utils` there, since it can't build without its nightly. Run
  `cargo dylint upgrade lints`, which moves `lints/rust-toolchain.toml` and `clippy_utils`
  together, fix what the newer compiler internals break, and set `lints_nightly` in
  `template/justfile.jinja` to match. If the new nightly needs a newer cargo-dylint, move
  cargo-dylint and dylint-link with it, as below. A newer compiler can word or place a report
  differently; rewrite the `.stderr` files it changes as step 4 of [Add a lint](#add-a-lint) says.
- **The rustfmt nightly**: `nightly` in `justfile`, `template/justfile.jinja` and the Editors
  section of `template/CONTRIBUTING.md.jinja`.
- **cargo-dylint and dylint-link**, at the version of `dylint_linting` in `lints/Cargo.toml`.
- **Inno Setup**, which builds a generated binary's Windows installer: the URL and SHA-256 of its
  x64 installer in the `Install Inno Setup` step of
  `template/.github/workflows/build-release.yml.jinja`. Take both from the release's assets on
  [GitHub](https://github.com/jrsoftware/issrc/releases) and check the digest against the one
  [jrsoftware.org](https://jrsoftware.org/isdl.php) lists. A new major version needs the
  6-to-7-style upgrade test in `tests/installer/run.sh` moved to it, and the generated docs'
  description of what users see. `tests/installer/run.sh` also pins Inno Setup 6.7.4, the version
  before trellis pinned one, which stays.
- **The check tools**, on the workflows' `tool:` lines and in `install-tools` in
  `template/justfile.jinja`; just and shellcheck in the Set up section of
  `template/CONTRIBUTING.md.jinja`, and just also in this file's Set up section, `README.md`,
  `copier.yml` and `template/docs/maintaining.md.jinja`; release-plz's `version:` inputs and the
  `tool@x.y.z` mentions in the comments of `template/.github/workflows/`; actionlint's `go install`
  in `template/justfile.jinja` and the workflows; and copier's version in `justfile` and
  `README.md`.

`just pins` fails until every copy of a version agrees, including a copy in a file it hasn't seen
before.

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
   request. List what changed, and as list items under "Action required in projects", what an
   owner must do before or after the update merges. `just changelog` checks that the heading is
   the version after the newest tag by the rule above, counting those list items as asks. To
   gather notes before you release, head the section `## Unreleased`, which releases nothing, and
   rename it when you release. A release that moves the stable toolchain also moves projects'
   `rust-version`, so list under "Action required in projects" that a published library notes
   the new minimum Rust version in its changelog and makes its next release a minor one.
2. Merge it. Once every other CI job passes on that commit, CI's `Release` job tags it with an
   annotated tag and publishes a GitHub release with the section as its notes. Renovate opens the
   update in every project. Only a commit that changes the section gets the tag, so a later
   commit whose CI passes first can't release changes the notes don't list. If CI fails on the
   commit, rerun it once the cause is fixed, or change the section again in the pull request that
   fixes it. A rerun on a commit the job already tagged publishes a release it missed and tags
   nothing.

## Send a change

Pull requests are squash-merged, so the title becomes the commit on `main` and must be a
[conventional commit](https://www.conventionalcommits.org/), such as
`feat(lints): flag wildcard re-exports`.
