# Changelog

What each trellis release changes in the projects it generates, newest first. Each section is
headed with its tag, or with `Unreleased` until it's released. Its "Action required in projects"
part lists what a project's owner must do around the update: a rule that can flag existing code,
a setting to change, or a step to do by hand. [CONTRIBUTING.md](CONTRIBUTING.md#release-trellis)
has the versioning rule.

## v0.11.0

### Action required in projects

- **Keep every pair of paths apart on macOS and Windows.** `just policy` now fails two paths in
  the repository that macOS or Windows reads as one, such as `docs/Install.md` beside
  `docs/install.md`, `É.txt` beside `é.txt`, an `é` written as one character beside one written as
  `e` and a combining accent, two crate directories whose names differ only in letter case, or a
  directory spelled `crates` with a long s (U+017F) beside `crates/`. A checkout on a Mac or on
  Windows holds only one of them, so it can build files no check read. A project generated from
  trellis has none. Before the update, delete or rename one of each pair. Under `crates/`, or
  under a `crates` spelled with a long s, that is a code pull request. Elsewhere it is a rules pull
  request, or neither for a page under `docs/` other than `docs/style.md`. Of two crate
  directories that differ only in case, rename one that holds no rule file, such as a `build.rs` or
  a nested `.gitignore`, in a code pull request with the `path` of each dependency on it. Then fix
  its row under Layout in `CONTRIBUTING.md` in a rules pull request. If both hold one,
  `just rule-changes` refuses the rename, so turn the ruleset off for that one merge and run
  `just setup-github` afterward.
- **Rename any path that holds U+0345.** `just policy` now fails a path that holds the combining
  Greek ypogegrammeni (U+0345), which macOS and Windows fold in different orders. A project
  generated from trellis has none. Before the update, rename or delete it, in a code pull request
  under `crates/` and a rules pull request elsewhere.
- **Give every crate directory a name Windows reads as written.** `just policy` now fails a
  `crates/<name>` whose name ends in a dot or a space, or holds a `:` or a `\`. A project
  generated from trellis has none. Under the checker before this update, every path in such a
  directory is a rule and every path in the renamed one is code, so the rename always mixes them.
  If no crate depends on it and it has no build script, delete the directory in a rules pull
  request, leaving its entries in `internal_dependencies` and in `changelog_include` of
  `release-plz.toml`. Add it back under a plain name in a code pull request, without any nested
  `.gitignore`, `.ignore`, `tsconfig.json`, `bunfig.toml` or `.npmrc`. Then add those files back,
  and change its row under Layout in `CONTRIBUTING.md`, in a rules pull request. Otherwise turn
  the ruleset off for the one merge that renames it and fixes each dependency's `path`, before
  the update, and run `just setup-github` afterward.
- **Point each TypeScript settings file only at other settings files.** Every `.json` file whose
  name starts with `tsconfig`, such as `tsconfig.base.json` or Vite's `tsconfig.app.json`, is now
  a rule wherever it sits. `just policy` fails one whose `extends` or `references` names anything
  but another such file in the repository, by a path that starts with `./` or `../`, holds no `\`
  and stays outside `node_modules`. A `references` path may also name a directory that holds a
  `tsconfig.json`. A package such as `@tsconfig/strictest` fails too, as does a settings file that
  is a symbolic link or that TypeScript could read otherwise. A file TypeScript could read
  otherwise is one that isn't JSON with comments and trailing commas in UTF-8, such as one with
  single-quoted strings or unquoted keys, or one that holds a lone carriage return, U+0085,
  U+2028 or U+2029, a block comment never closed, a comma with nothing before it, or a key given
  twice. A project generated from trellis has none of these. Before the update, fix them under
  the checker you have, which counts only a file named `tsconfig.json` as a rule. For a
  `tsconfig.json` under `crates/`, copy the file `extends` names, or the `compilerOptions` of the
  package it names, to a `tsconfig.base.json` beside it in a code pull request. Point `extends`
  at the copy in a rules pull request, then delete the old file in a code pull request. Keep the
  copy beside the `tsconfig.json`, because TypeScript reads `include`, `files` and the other paths
  in a settings file relative to that file. For a settings file under `crates/` with another
  name, such as `tsconfig.app.json`, copy and point in one code pull request. Outside `crates/`
  the old file is a rule already, so rename it and point `extends` at it in one rules pull
  request. Fix a settings file's syntax in a rules pull request for a `tsconfig.json`, and in a
  code pull request for one under `crates/` with another name.

### Changed

- A crate's directory can move in a code pull request. When `crates/<old>` at the base and
  `crates/<new>` at the head hold the same files, byte for byte, with the same attributes, and no
  other directory is in the way on any file system, `just rule-changes` counts the crate's files as
  neither rules nor code. So a crate with a build script or a nested `.gitignore` can move beside
  the `path` of each dependency on it. Change its row under Layout in `CONTRIBUTING.md` in a rules
  pull request after it. A crate that root files name, as the `justfile`, `Containerfile`,
  `.gitignore` and `.dockerignore` name a service's web UI crate, moves with those files in one
  rules pull request. A file in the crate that reads its own location, such as a build script
  that reads `CARGO_MANIFEST_DIR`, can then do something else, as a code change can already change
  what a listed build script emits. The judgment needs git 2.40 or later. With an older git, a
  move is judged file by file as before.
- `build_script_crates` may name a package that no crate has, so renaming the package of a crate
  with a build script takes three pull requests that each pass: a rules pull request that adds the
  new name beside the old one in `internal_dependencies`, `changelog_include` and
  `build_script_crates`, a code pull request that renames the package, and a rules pull request
  that drops the old name. An entry for a member with no build script still fails. Nothing removes
  a stale entry, and a crate that later takes the name can add a build script in a rules pull
  request without a list line.
- `docs/maintaining.md` says how to rename a crate, which root files name a crate's directory,
  and that renaming the package that carries the project's name still changes rules and code
  together.
- Renaming a crate in the layout trellis generates no longer needs the `main` ruleset turned off.
  That step remains only in the action items above and in those of v0.8.0 and v0.10.0, for
  layouts that `just policy` now refuses and that trellis never generated.

## v0.10.0

### Action required in projects

- **Give every crate directory an ASCII name.** `just policy` now fails a `crates/<name>` whose
  name holds a character outside ASCII. A project generated from trellis has none. Before the
  update, rename the directory and the `path` of each dependency on it in a code pull request,
  then change its row in the Layout table of `CONTRIBUTING.md` in a rules pull request. A crate
  with a build script can't move that way. Its `build.rs`, or the file its `build` key names, is
  a rule and the rest of the crate is code, so `just rule-changes` refuses the move and the
  `main` ruleset has no bypass. Turn the ruleset off for that one merge, before the update, and
  run `just setup-github` afterward.
- **Name a build script in ASCII, with no part that ends in a dot or a space.** `just policy`
  now fails a crate that `build_script_crates` lists when its `build` key holds a character
  outside ASCII, or a part that ends in a dot or a space, which Windows removes from the end.
  Rename the file and the key in a rules pull request before the update.
- **Rename any path Windows reads as another path, before the update.** `just rule-changes` now
  counts a path with a part that ends in a dot or a space, or that holds a `:` or a `\`, as a
  rule, so a pull request that changes one can't also change code. After the update, renaming
  it away changes a rule and code together. Rename it in a code pull request before the update.
  A project generated from trellis has none.

### Changed

- Rules that go by a file's name match nine more characters that macOS reads as ASCII letters: the
  sharp s in either case as `ss`, and the ligatures U+FB00 to U+FB06 as `ff`, `fi`, `fl`, `ffi`,
  `ffl` and `st`. A file whose name spells `rustfmt.toml`, `justfile` or `tsconfig.json` with one
  of them is that file on a Mac. Under `crates/`, `just rule-changes` counted it as code. Now it
  is a rule, and `just policy` fails a tool's settings file or an agent instructions file spelled
  that way, as it fails the plain spelling there.
- `just rule-changes` counts a path that Windows reads as another path as a rule: one with a part
  that ends in a dot or a space, or that holds a `:` or a `\`. Git for Windows refuses to check
  such a path out while its `core.protectNTFS` setting is on, as it is by default, but the path
  still reaches every other checkout.
- `just policy` fails a crate directory whose name isn't ASCII, and a `build` key that isn't ASCII
  on a crate that `build_script_crates` lists. File systems disagree on which non-ASCII names are
  one name, so no check can tell which files they are everywhere. It also fails a `build` key
  with a part that ends in a dot or a space, which names another file on Windows.

## v0.9.0

### Action required in projects

- **Remove any git submodule under `crates/` before the update.** `just policy` now fails a git
  submodule, or an untracked git repository that `git add` would make one, under `crates/` or in
  or above a crate's directory. Copy the files you need into the repository, or depend on the
  repository through Cargo as a git dependency. A project generated from trellis has none. The
  checker your project pins until the update counts a submodule under `crates/` as code and
  `.gitmodules` as a rule, so removing both in one pull request fails `just rule-changes`. Remove
  the submodule in a code pull request with `git rm --cached <path>`, which leaves `.gitmodules`
  as it is, then drop its entry from `.gitmodules` in a rules pull request. `git rm --cached`
  leaves the submodule's files on your disk with their `.git`. Delete that `.git`, or the whole
  directory, or `just policy` fails it as an untracked git repository.
- **Keep a build script in its crate's directory.** `just policy` now fails a crate that
  `build_script_crates` lists when its `build` key names a file outside the crate's directory,
  such as `build = "../../tools/gen.rs"`, or holds a `\`. Move the file into the crate and write
  the path with `/`.

### Changed

- `just policy` fails a git submodule, or an untracked git repository, under `crates/`. It lists
  files through git, which shows none of a submodule's files, and `just rule-changes` counted a
  submodule under `crates/` as code. So a submodule could hold a crate whose build script,
  manifest and tool settings changed with no rule seeing them.
- `just rule-changes` counts a git submodule as a rule wherever it sits, so moving one to another
  commit can't land beside code. It counted one under `crates/` as code, whatever the new commit
  changed inside it. It reads the move even when `.gitmodules` or your git settings tell
  `git diff` to ignore submodules.
- `just policy` fails a crate in `build_script_crates` whose `build` key leaves the crate's
  directory. Outside it, the path could pass through a symbolic link back to one of the crate's
  modules, and a change to that module would then change the build script as code. It also fails a
  `build` key that holds a `\`, which names one file on Windows and another elsewhere.
- `just docs` and the doctests fail on a rustdoc warning even when `CARGO_ENCODED_RUSTDOCFLAGS` is
  set in your shell. Cargo reads that variable before `RUSTDOCFLAGS`, so setting it, even empty,
  turned the errors back into warnings. The justfile now exports the flag through it. CI never
  set it.

## v0.8.0

### Action required in projects

- **Run `just install-tools` after the update, then `cargo clean` once.** CI and `install-tools`
  now pin cargo-dylint and dylint-link 6.4.0. With 6.3.0 installed, `just dylint` can pass on your
  machine with lint violations present, after any run of the lints without `-D warnings`. A check
  that 6.3.0 cached stays cached after the upgrade until the crate's sources change, which is what
  the `cargo clean` clears.
- **Inherit the workspace license in every crate.** When the root `Cargo.toml` has
  `[workspace.package] license`, `just policy` now fails a crate whose `[package]` lacks
  `license.workspace = true` or sets a license of its own. Generated crates have the line, unless
  the project was generated with license `none` and licensed later. Add it to any crate without
  it, in a code pull request before the update, since the rule-change check of the update's pull
  request still runs the old checker.
- **Keep every crate in `crates/<name>`.** `just policy` now fails a workspace member outside
  `crates/<name>`, such as `crates/a/b` or a path dependency outside `crates/`. It also fails a
  `crates/<name>` or `crates` that is a symbolic link, and a package in `crates/<name>` that isn't
  a member. A project trellis generated already has this layout. If you added a crate elsewhere,
  moving it changes rules and code in one pull request, which `just rule-changes` refuses and the
  `main` ruleset has no bypass for. Turn the ruleset off for that one merge, before the update,
  and run `just setup-github` afterward.
- **Land nested settings files as rules.** A `.gitignore`, `.ignore`, `tsconfig.json`,
  `bunfig.toml` or `.npmrc` anywhere in the repository, such as a web UI's `tsconfig.json`, is now
  a rule, so a pull request that changes one can't also change code.
- **Install shellcheck, if you haven't.** `just actionlint`, and so `just ci`, now fails without
  it. The Set up section of `CONTRIBUTING.md` names the version.

### Changed

- A new project's first `just ci` passes before its GitHub repository exists. `just rule-changes`
  compares with `origin/main`, which doesn't exist until the first push, so it failed with
  `Not a valid object name origin/main` and trellis's README put creating the repository, setting
  it up and pushing ahead of any local check. It now says there is nothing to compare yet and passes
  when its base isn't a commit and the repository has no remote-tracking branches, as with no
  remote or one that nothing was pushed to. Once any remote branch exists, an unknown base still
  fails. trellis's README and the steps Copier prints install the tools and run `just ci` before
  "Before the first push", and the README's generation command pins the Copier version trellis is
  tested with.
- `just dylint` fails whenever the code holds a lint violation, whatever ran before it. cargo-dylint
  6.3.0 passed the recipe's `-D warnings` to the compiler without telling cargo, so after a plain
  `cargo dylint` or `just _dylint ""` cargo kept that run's checks, and `just dylint` printed their
  warnings again and passed. cargo-dylint 6.4.0 tells cargo, which reruns the lints when their
  levels change. CI's fresh checkouts weren't affected; a local `just ci` was. The recipe is
  unchanged. 6.4.0 also has a download for Windows, so CI's Windows jobs install it in seconds
  instead of building it for about four minutes.
- `trellis-policy check` reads `path = "..."` as a module's file only in a `path` attribute
  (`#[path = ..]`, `#![path = ..]`) or as an attribute a `cfg_attr` applies, and still finds those
  inside macro arguments and inside other attributes' arguments. A log field such as
  `tracing::info!(path = "/health", "served")`, a named `format!` argument called `path`,
  `#[utoipa::path(get, path = "/users")]` or askama's `#[template(path = "hello.html")]` no
  longer fails `member_sources`.
- A `[[bench]]` may set `harness = false`, which Criterion and divan need. `just test` runs no
  bench, and `just lint` and `just dylint` still check it. `harness` stays rejected on `[lib]`,
  `[[bin]]`, `[[test]]` and `[[example]]`, and `crate-type` stays rejected.
- `trellis-policy rule-changes` no longer counts a `build.rs` below a crate's `src/`, `tests/`,
  `benches/` or `examples/` as a build script. A module named `build`, such as
  `crates/<name>/src/build.rs`, or a test named `tests/build.rs` is code, so it lands with the
  code that uses it. That holds only while the crate's directory holds no `Cargo.toml` other than
  its own, anywhere below it and in any letter case, before or after the change, since a package
  nested there runs its `build.rs` as a build script. Every other `build.rs` is still a rule, and
  so is the file a crate's `build` key names, such as `build = "gen.rs"`. The modules a build
  script reads stay code.
- `trellis-policy rule-changes` counts a crate's `[package] license` as a rule setting, beside
  `build` and `[build-dependencies]`, so the pull request that changes the license answer to or
  from `none` carries each crate's line and stays a rule change.
- An image under `docs/` (`png`, `svg`, `jpg`, `jpeg`, `gif` or `webp`) is neither a rule nor code,
  like the pages, so a page and its picture land together with code. Other files no list knows
  are still rules. A web UI's `package.json`, its scripts and its sources stay code, and the
  generated `CONTRIBUTING.md` says so.
- When a local `just rule-changes` lists an untracked file that is a rule only because no list
  knows it, such as a new `.envrc`, it says so and how to fix it:
  `.envrc (untracked; commit it in a rules pull request, or ignore it in .git/info/exclude)`.
  The file still counts.
- `copier update --data publish_crate=true` on a library with no license now refuses until you
  also pass a license, such as `--data license=MIT`. Before, it switched the license answer to MIT
  and wrote MIT license files without asking. `copier update --data visibility=private` on a
  published library now refuses until you also pass `--data publish_crate=false`. Before, it
  turned publishing off and deleted the crate's license files without asking, though v0.3.0's
  notes said it refused. Both refusals hold whether the answers come from `--data` or
  `--data-file`, and an answer you don't pass is never changed to get past them.
- "Change an answer" in `docs/maintaining.md` says, per answer, what an update can and can't do.
  It records a new `project_name` or `kind` and rewrites the files outside `crates/`, but the
  crates keep their names and scaffold. A new `description` reaches the README, `CONTRIBUTING.md`
  and a service's `Containerfile` label, not the crates. A license to or from `none` needs each
  crate's line in the same pull request. Turning on a service's `web_ui` needs the app landed
  first in a code pull request, with its `bun.lock` from `bun install --lockfile-only` and without
  its `tsconfig.json`, which goes in the update's pull request. Turning it off needs the app's
  `node_modules` and `dist` deleted before the checks. Adding Linux or Windows to a binary or
  library takes the default processors unless you pass `architectures` too.
- `file_named_after_item` no longer names a module after a private helper type. A module of public
  functions with one private struct, such as a `render.rs` holding `pub fn page` and
  `struct Buffer`, is one the Google guide lets you name as desired, and the lint now leaves it
  alone. Only visible types can be a module's primary item.
- `file_named_after_item` and `non_leaf_module_items` take an `expect` with a reason. The guide
  lets a leaf module that isn't about one abstraction be named as desired, and allows a little
  "ties things together" code in a non-leaf module, neither of which a lint can tell. An `expect`
  of `file_named_after_item` goes on the `mod name;` declaration, and one of
  `non_leaf_module_items` on the item it reports. `suppression_policy` rejects an `expect` of
  either on an item with a module beneath it, whose reports it would cover as well; a
  `#[cfg(test)]` module that holds no module may be beneath it.
- `doc_after_attribute` is listed with the lints that take no exception. It reads source text, so
  an `expect` on the item never reached it and failed as unfulfilled; `suppression_policy` now
  says why.
- Lint help texts and docs say only what the code does. `disabled_code` says that a plain
  `cargo test`, and an editor running a module's tests, skip an ignored test, while `just test`
  and CI run it. `impl_into_params` says trellis applies the rule to library APIs too, where a
  parameter whose call-site ergonomics are paramount takes an `expect`. `code_comment` says why
  comments are banned. The `suppression_policy` help for a lint that takes no exception names
  trellis's `template/docs/style.md`.
- A new binary gets `tests/command_line.rs`, which a service already had. It runs the built
  program and checks that `--version` prints the name and the version and exits 0, and that an
  unknown argument exits 2. The core crate's `tests/version_line.rs`, which only restated the
  function, is gone. An update never touches `crates/`, so existing projects keep their tests.
- A published library's release workflow no longer writes its own release-plz config. That step
  handles a crate added or renamed since the last tag, which only git_only mode trips on, and a
  published library doesn't use git_only mode.
- `docs/maintaining.md` says that a pull request merged before the release workflow's Tag job (a
  library's Release job) finishes can get no release pull request until the next push to `main`.
  release-plz doesn't propose the same version again, as the page used to say. It also says
  Renovate opens a trellis update in its weekly run, not as soon as trellis tags. A service's page
  says that an update that only moves the base images' digests publishes no new image; the new
  base images ship with the next release.
- A published library's `docs/stability.md` says its minimum Rust version is the stable Rust the
  release is built and tested with, raised only in a minor release.
- `just actionlint` fails when shellcheck isn't installed. actionlint skipped the workflows' shell
  scripts without a word when shellcheck was missing, so a local `just ci` could pass what CI,
  which installs shellcheck, fails.
- Comments now say what the checks do: the trusted-macro note in `dylint.toml` (the versions are
  the ones trellis's tests pin, and what `just dylint` does and doesn't see after a bump), the
  header of `rule-changes.yml` (any workflow run in the repository can set the status), and the
  comments in `rustfmt.toml` and the `justfile` on what the Google guide asks of rustfmt. No
  setting changed.

## v0.7.2

### Changed

- A service's image makes its license files readable by everyone. The final stage copied them
  with the modes they had in the checkout and from `just notices`, so on a machine whose umask is
  077 they landed as root-owned 0600 files that the image's non-root user couldn't read. Images
  built on CI's runners, whose umask is 022, weren't affected. The build stage now installs them
  with mode 644 into `/licenses`, and the final stage copies that directory. If you edited the
  final stage's `COPY` line for `/usr/share/doc/<name>/`, Copier can leave conflict markers there;
  keep the new `COPY --from=build /licenses/` line, and add any file you copied to the build
  stage's `install` line.

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
