# Changelog

What each trellis release changes in the projects it generates, newest first. Each section is
headed with its tag, or with `Unreleased` until it's released. Its "Action required in projects"
part lists what a project's owner must do around the update: a rule that can flag existing code,
a setting to change, or a step to do by hand. [CONTRIBUTING.md](CONTRIBUTING.md#release-trellis)
has the versioning rule.

## v0.4.0

### Action required in projects

- **Run `just setup-github`, then `just check-github`, on `main` after the update merges.** It
  needs `gh` signed in as an admin of the repository. It applies the settings
  `docs/maintaining.md` describes, which earlier releases left to pasted `gh` commands. If you
  already ran it on the update branch, as the next item asks of some public repositories, running
  it again sets the same values and creates nothing twice.
- **In a public repository without the Actions policy, run `just setup-github` first.** From
  2026-12-02, GitHub runs `pull_request_target` workflows in a public repository only when an
  Actions policy allows them. If you never added the policy that lets `rule-changes.yml` run, as
  earlier releases' `CONTRIBUTING.md` asked, the rule-change check doesn't run from that date on,
  so no pull request, the update included, gets the "Rule changes stand alone" status. Run
  `just setup-github` on the update branch, whose justfile has the recipe, before you open the
  update pull request. If it is already open, edit its title or description afterwards to rerun
  the check.
- **Switch branch protection to the `CI passed` job.** CI now ends with one job, `CI passed`, that
  passes only when every other CI job passed. `main` requires `PR title`, `CI passed` and "Rule
  changes stand alone" instead of each job by name. `just setup-github` makes the switch: it adds
  a ruleset with those checks and removes the checks classic branch protection required.
- **Fix what the new guards report.** CI fails when the visibility answer is public and the
  repository isn't, and the release pull request job fails when the `RELEASE_PLZ_TOKEN` secret is
  empty. Each error says what to do, and `just check-github` reports both.

### Changed

- `docs/maintaining.md` holds the owner's work: setting up the repository, releasing, changing
  an answer, updating trellis and renewing the release token. `CONTRIBUTING.md` keeps what
  contributors need.
- `just install-tools` installs the check tools at the versions CI pins.
- `just setup-github` sets squash merging only, with the pull request's title and description as
  the commit, the `release` label and a ruleset that protects `main`, and in a public repository
  the Actions policy for `rule-changes.yml` and private vulnerability reporting.
  `just check-github` reports each setting as ok or wrong. Copier prints the next steps after it
  generates a project.
- The pull request template drops the boxes for `just ci` passing and for not mixing rules and
  code, which the required `CI passed` and "Rule changes stand alone" checks already enforce.
- A new binary or service answers `--version` and fails on other arguments. A new service's
  `tests/command_line.rs` checks it, and its `AGENTS.md` states the contract that CI's
  `Container image` job relies on. An update leaves `crates/` alone, so the new `main.rs` and test
  reach only new projects. An existing service needs no change: the `Container image` job ran the
  image with `--version` in earlier releases too.
- In a published library, `AGENTS.md` says that a new crate needs a first manual publish and a
  trusted publisher on crates.io, which is the owner's work, and links the steps in
  `docs/maintaining.md`.
- "Update trellis" in `docs/maintaining.md` points to this changelog. It says Copier merges your
  edits to the files it renders, such as a crate's row in `AGENTS.md`, its settings in
  `Cargo.toml` and `release-plz.toml`, and a page's line in `README.md`, and leaves conflict
  markers where trellis changed the same line or one next to it. trellis's update test now covers
  those edits.
- `docs/style.md` notes that its links to trellis's lints and checker show trellis's `main`,
  while a project builds them at the commit `trellis_rev` in its `justfile` names.
- `docs/stability.md` lists the names that stay the same, which users' scripts and verify commands
  depend on. The release steps ask for upgrade notes under each breaking change.
- An update delivers a published crate's `LICENSE` files to follow the license answer, so the
  manual copy step is gone.
- `docs/style.md` holds each rule once, with a new "Silencing a lint" section. `AGENTS.md` and
  `CONTRIBUTING.md` summarize the rules and link to it.
- The binary kind's wording fits a desktop app as well as a command-line tool.
- What a private repository costs and lacks is in one place, "Change an answer" in
  `docs/maintaining.md`.

## v0.3.0 and earlier

These releases have no notes. `git log` between their tags lists what changed.
