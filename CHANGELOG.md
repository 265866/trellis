# Changelog

What each trellis release changes in the projects it generates, newest first. Each section is
headed with its tag, or with `Unreleased` until it's released. Its "Action required in projects"
part lists what a project's owner must do around the update: a rule that can flag existing code,
a setting to change, or a step to do by hand. [CONTRIBUTING.md](CONTRIBUTING.md#release-trellis)
has the versioning rule.

## v0.7.1

### Changed

- `suppression_policy` allows `#[cfg_attr(not(test), expect(dead_code, reason = "..."))]`, so
  private code that only its tests use can land before its first caller. A plain
  `expect(dead_code, ..)` went unmet in the test build, and no other form passed. The predicate
  must be exactly `not(test)`, and the `expect` must name `dead_code` alone. `just policy` counts
  it as a lint expectation, so a rule update's pull request may add or remove it.

## v0.7.0

### Action required in projects

- **Choose the processors a service's image runs on.** Services now get the `architectures`
  question too, with the same defaults: x86-64 and ARM64 for a public repository, x86-64 for a
  private one. So the update makes a public service's release publish one image for
  `linux/amd64` and `linux/arm64`, and its CI test and run the image on ARM64. To keep amd64 only,
  update with `--data 'architectures=[x86_64]'`. To add ARM64 to a private service, such as one
  that runs on a Raspberry Pi, update with `--data 'architectures=[x86_64, aarch64]'`.
- **Build another architecture's libraries in the `Containerfile`.** The build stage now runs on
  the builder's own platform and cross-compiles for each other architecture with Debian's cross
  gcc, so no architecture builds under emulation. If you added a build-stage step that installs a
  C library, such as `libssl-dev`, Copier can leave conflict markers there; install the target
  architecture's version instead, such as `libssl-dev:$TARGETARCH` after
  `dpkg --add-architecture $TARGETARCH`. A crate that finds the library with pkg-config also needs
  `PKG_CONFIG_ALLOW_CROSS=1` and `PKG_CONFIG_PATH` set to the target's Debian directory, such as
  `/usr/lib/aarch64-linux-gnu/pkgconfig`, when it cross-compiles.

### Changed

- The `Containerfile` cross-compiles: its build and web stages run on the builder's platform, and
  the final stage only copies files, so one x86-64 runner builds every architecture's image.
  `just image` still builds for your own machine.
- CI's `Container image` job builds every architecture's image the way the release does and
  uploads them, and a new `Run the image` job starts each one on a runner of its architecture.
- `docs/install.md` names the architectures the image runs on, and `docs/maintaining.md` shows how
  to change a service's architectures.

## v0.6.0

### Action required in projects

- **Choose the processors, or keep ARM64.** A new question, `architectures`, picks x86-64, ARM64
  or both for Linux and Windows; macOS stays Apple silicon only. It defaults to both for a public
  repository and to x86-64 for a private one, whose ARM64 runners draw on the account's Actions
  usage. So the update adds Linux and Windows on ARM64 to a public project's CI and releases. To
  keep x86-64 only, update with `--data 'architectures=[x86_64]'`.
- **Point Linux downloads at the musl archives.** A binary's Linux releases are now built for
  musl and linked statically, as `<name>-<version>-x86_64-unknown-linux-musl.tar.gz`, instead of
  `x86_64-unknown-linux-gnu`. The glibc build needed the glibc of the Ubuntu runner that built it,
  so it didn't start on Alpine or on older distributions such as Debian 11; the musl build runs on
  any Linux. Change any script, package manifest or doc that downloads the Linux archive by name.
  If the binary has released before, the new name breaks a name `docs/stability.md` says no
  release changes, so make its next release a breaking one. release-plz counts only commits under
  `crates/`, which an update never touches, so a `!` on the update's pull request doesn't do it.
  Instead, on the next release pull request's branch, set the next minor version (major from 1.0)
  in the root `Cargo.toml`, `Cargo.lock` and the `CHANGELOG.md` heading, and add an entry that
  says the Linux archive is now `<name>-<version>-x86_64-unknown-linux-musl.tar.gz`, as steps 1
  and 2 of "Each release" in `docs/maintaining.md` describe.
- **Build C code for musl.** A binary's CI now runs its tests and release build for musl on Linux,
  and installs `musl-gcc` for crates that compile C code. If such a crate needs more, such as a
  system library, add it to the musl build.

### Changed

- CI's build jobs are named for their target, such as `Linux on x86-64`, instead of `Ubuntu`,
  `macOS` and `Windows`. `CI passed`, the check `main` requires, is unchanged.
- The build jobs run the tests and the release build for the target a release ships, and the
  `targets` lists in `deny.toml` and `about.toml` follow it.
- `docs/install.md` lists one archive per target, and `CONTRIBUTING.md`'s Platforms section names
  each target.
- The `disabled_code` help no longer says CI builds one target per operating system. It says
  `ci_targets` lists only operating systems, which is why `target_arch` and `target_env` stay
  rejected.

## v0.5.0

### Action required in projects

- **Move your edits out of `AGENTS.md`.** `AGENTS.md` is now a title and a link to
  `CONTRIBUTING.md`, which holds the rules `AGENTS.md` had: the layout table, product boundaries,
  where docs go, the settings a new crate needs, platforms, the code rules, comments, silencing a
  lint and where a rule belongs. If you edited `AGENTS.md`, such as adding a crate's row to its
  layout table, Copier can leave conflict markers there. Take trellis's `AGENTS.md`, and add your
  lines to the matching section of `CONTRIBUTING.md` in the update pull request.
- **Replace a release token that lacks access.** The release workflow's "Update the release PR"
  job now runs `just check-release-token` before release-plz. It fails and says why when the
  `RELEASE_PLZ_TOKEN` secret has expired, been revoked or been mistyped, has no access to the
  repository, belongs to an account that can't push, or can't write contents or pull requests. It
  warns in the token's last 14 days. If the job fails after the update, store a new token with
  `just set-release-token`. `docs/maintaining.md` says which access the token needs, under
  "Before the first push".
- **Stop agents from merging in the agent tool, if you relied on `AGENTS.md` for it.**
  `AGENTS.md` no longer tells agents not to merge pull requests, push to `main` or run
  `just setup-github`. The `main` ruleset still blocks pushes and requires checks, and the owner's
  recipes still need admin rights, but whether an agent may merge is now only the agent tool's
  setting.

### Changed

- `just set-release-token` reads a token without echoing it, runs the same check, and stores the
  secret only if the token passes. `docs/maintaining.md` and the hints `just setup-github` prints
  use it instead of `gh secret set`.
- A service can build a web UI with Bun. A new question, `web_ui`, asked only for services and off
  by default, adds a TypeScript starter in `crates/<crate>/web`; the `web-install`, `web-check`,
  `web-build`, `web-audit` and `web-notices` recipes, with `web-check` and `web-build` in
  `just ci` and `web-audit` in `just deny`; a Bun stage in the `Containerfile` that puts the build
  in `/usr/share/<project>/web` and names it in `<CRATE>_WEB_DIR`; Bun in CI's `Quick checks` and
  `Container image` jobs, the release workflow's image job and the Advisories workflow; and
  Renovate's `bun` manager. The image's license notices also list every installed npm package, and
  fail on a license `about.toml` doesn't accept.
- A project without a web UI renders as before. An update never writes into `crates/`, so an
  existing service that turns `web_ui` on, as "Change an answer" in `docs/maintaining.md` says,
  gets the recipes and CI but brings its own app.
- The before-commit review no longer asks for a fresh-context review subagent. It says to review
  the diff against the checklist in `docs/style.md` and the Code section of `CONTRIBUTING.md`.

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

## v0.3.0

### Action required in projects

- **In a private repository, change the visibility answer to private.** Copier now asks whether
  the repository is public or private. The default, public, keeps an existing project as it was.
  A binary's or service's release in a private repository stopped at the attestation step,
  because GitHub attests only in public repositories unless the account has Enterprise Cloud.
  Before the next release, merge the change that
  `uvx copier update --defaults --vcs-ref=:current: --data visibility=private` makes. A published
  library can't be private, because a private library can't publish to crates.io. Copier refuses
  the answer until you also pass `--data publish_crate=false`.

### Changed

- The archives a binary's release workflow passes from its build jobs to its publish job expire
  after 7 days.
- A private project's releases skip attestation, and a service's image isn't signed, because a
  keyless cosign signature would name the private repository in Sigstore's public log. Binaries
  still ship `SHA256SUMS`, and the image still gets its version, minor and `latest` tags.
- A private project's `CONTRIBUTING.md` drops the setup steps only a public repository needs. It
  says that protecting `main` needs GitHub Pro, and that macOS and Windows minutes cost more.
- In a private project, security reports go in issues, and the issue templates no longer link to
  private advisories.
- A private project's install docs explain how to sign in to `ghcr.io` with a classic
  `read:packages` token and how to fetch the repository with Git credentials.
- A private library can't publish to crates.io, and Copier says so.
- Except in a published library, `CONTRIBUTING.md` says how to switch between public and private.
  That includes deleting a service's public `ghcr.io` package before it goes private, because
  GitHub can't make a public package private again.

## v0.2.0

### Action required in projects

- **Fix what the new `docs_layout` rule and the stricter `docs_index` rule report.**
  `just policy` checks a 12th rule, `docs_layout`. The files in `docs_files` must exist.
  `README.md` must have one `#` title and exactly the `##` sections in `readme_sections`, in
  order. Its Documentation section must hold only `- [Title](path): description.` entries, each
  description ending in `.`. Every page in `docs/` must open with a title, a blank line and a
  paragraph, and no HTML may be left open. `docs_index` now counts only links in the README's
  Documentation section, so a page linked only from elsewhere in the README fails, and a page in a
  subdirectory of `docs/` must be linked from a page in the directory above it. The update adds
  both settings to `[workspace.metadata.policy]` in `Cargo.toml`. A README or docs page you edited
  may fail until it has that shape.

### Changed

- `AGENTS.md` has a Docs section and `docs/style.md` a Documentation section. They cover a short
  README with fixed sections, one page per reader task linked from the index, the page shape and
  voice, where design decisions and compatibility promises go, and updating a page in the same
  pull request as the change.

## v0.1.0

The first release. It generates a binary, service or library workspace with trellis's custom
lints, `trellis-policy` and its 11 structure rules, CI and release workflows, and the docs and
agent rules a project starts with.
