# trellis

> [!WARNING]
> This is very opinionated, and it is built for one person's projects. You should not be using
> it. Read it, take the parts you like, and leave the rest.

trellis is a [Copier](https://copier.readthedocs.io/) template for new Rust projects. A project it
generates starts with:

- **Strict lints.** All of clippy's pedantic group, about 40 more rustc and clippy lints, and the
  custom lints in [`lints/`](lints). The custom lints check the
  [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html) rules clippy
  doesn't, and trellis's own rules, which
  [template/docs/style.md](template/docs/style.md#checked-by-tools) lists under "Beyond the Google
  guide". Every exception is an `expect` with a reason.
- **A structure checker.** [`trellis-policy`](policy) checks crate manifests, the dependencies
  between crates, where config files live, and the docs layout: the files and README sections
  each project keeps, the README's docs index, and how each page opens.
- **Rules apart from code.** A pull request changes the rules or the code, never both, so review
  sees each loosened rule apart from the code that might have needed it.
- **CI and releases.** Checks on Linux, macOS and Windows; release-plz versions and changelogs;
  release archives or a container image, attested and signed when the repository is public;
  Renovate for updates.
- **The same docs layout.** A short README with a docs index, task pages under `docs/`,
  `CONTRIBUTING.md`, `SECURITY.md`, and one `AGENTS.md` for coding agents.

The rules are written for code that coding agents write and that nobody reads line by line. So a
rule lives in the types or in a check wherever one can hold it, and review is the last resort. An
agent copies whatever pattern it finds in the repository, a workaround included. So every exception
carries a written reason where review sees it.

[template/docs/style.md](template/docs/style.md) is the generated projects' style guide. It lists
every rule and what checks it.

## Generate a project

You need [uv](https://docs.astral.sh/uv/), [rustup](https://rustup.rs/) and
[cargo-binstall](https://github.com/cargo-bins/cargo-binstall), and [gh](https://cli.github.com/) to
set up the repository. Install the [just](https://just.systems/) version that generated projects
pin; an older one can't read their `justfile`:

```sh
cargo binstall just@1.59.0
uvx copier@9.19.1 copy https://github.com/265866/trellis.git my-project
```

It asks for a name, a description, what the project ships (a binary, a container service or a
library), whether the repository is public or private, the platforms and processors, whether a
service has a web UI built with Bun, what a binary's builds need, and a license, then prints the
next steps. Commit the project,
install the tools that the generated `CONTRIBUTING.md` lists, and run every check:

```sh
cd my-project
git init --initial-branch main
git add -A
git commit -m "chore: generate from trellis"
just install-tools
just install-toolchains
just ci
```

Until the first push, `just ci` has no `origin/main` to compare the changes with, so its last
check says there is nothing to compare yet and passes. Then follow the "Before the first push"
section of the generated `docs/maintaining.md`. It creates the repository on GitHub, runs
`just setup-github` to set it up, and pushes. On Windows, run `just setup-github` and
`just check-github` in Git Bash. Create the repository with the visibility you answered. A private
project's releases aren't attested or signed, and the generated `docs/maintaining.md` says what
else a private repository changes.

## Update a project

```sh
uvx copier update --defaults
```

Copier merges the newer template into the project and keeps your changes. Conflicts show as merge
markers. Renovate opens the same update as a pull request in its weekly run after trellis tags a
release.
[CHANGELOG.md](CHANGELOG.md) says what each release changes and what it asks of a project's owner.

## What's here

| Path | Holds |
| --- | --- |
| [`template/`](template) | The files a generated project gets. [`copier.yml`](copier.yml) has the questions. |
| [`lints/`](lints) | The custom lints, a [dylint](https://github.com/trailofbits/dylint) library. |
| [`policy/`](policy) | `trellis-policy`, the structure and rule-change checker. |
| [`template/docs/style.md`](template/docs/style.md) | The generated projects' style guide, which the lints and the checker enforce. Each project gets it as its `docs/style.md`. It describes a project's checks, not trellis's. |

[CONTRIBUTING.md](CONTRIBUTING.md) covers working on trellis itself.

## License

[MIT](LICENSE).
