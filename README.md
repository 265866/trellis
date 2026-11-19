# trellis

> [!WARNING]
> This is very opinionated, and it is built for one person's projects. You should not be using
> it. Read it, take the parts you like, and leave the rest.

trellis is a [Copier](https://copier.readthedocs.io/) template for new Rust projects. A project it
generates starts with:

- **Strict lints.** All of clippy's pedantic group, about 40 more rustc and clippy lints, and the
  custom lints in [`lints/`](lints) for the
  [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html) rules clippy doesn't
  check. Every exception is an `expect` with a reason.
- **A structure checker.** [`trellis-policy`](policy) checks crate manifests, the dependencies
  between crates, where config files live, and the docs layout: the files and README sections
  each project keeps, the README's docs index, and how each page opens.
- **Rules apart from code.** A pull request changes the rules or the code, never both.
- **CI and releases.** Checks on Linux, macOS and Windows; release-plz versions and changelogs;
  release archives or a container image, attested and signed when the repository is public;
  Renovate for updates.
- **The same docs layout.** A short README with a docs index, task pages under `docs/`,
  `CONTRIBUTING.md`, `SECURITY.md`, and one `AGENTS.md` for coding agents.

[docs/style.md](docs/style.md) lists every rule and what checks it.

## Generate a project

You need [uv](https://docs.astral.sh/uv/) and [rustup](https://rustup.rs/).

```sh
uvx copier copy https://github.com/265866/trellis.git my-project
```

It asks for a name, a description, what the project ships (a binary, a container service or a
library), whether the repository is public or private, the platforms and a license. Then put it on
GitHub, install the tools that the generated `CONTRIBUTING.md` lists, follow its "Set up once per
repository" list, and run every check:

```sh
cd my-project
git init && git add -A && git commit -m "chore: generate from trellis"
gh repo create 265866/my-project --public --source . --push  # or --private
just install-toolchains
just ci
```

`just ci` compares the working tree with `origin/main`, so it needs the pushed `main`. Create the
repository with the visibility you answered. Without GitHub Enterprise Cloud, GitHub attests
release artifacts only in public repositories, and it takes private vulnerability reports only in
public ones, so a private project's releases aren't attested or signed, and its security reports go
in issues.

## Update a project

```sh
uvx copier update --defaults
```

Copier merges the newer template into the project and keeps your changes. Conflicts show as merge
markers. Renovate opens the same update as a pull request when trellis tags a release.

## What's here

| Path | Holds |
| --- | --- |
| [`template/`](template) | The files a generated project gets. [`copier.yml`](copier.yml) has the questions. |
| [`lints/`](lints) | The custom lints, a [dylint](https://github.com/trailofbits/dylint) library. |
| [`policy/`](policy) | `trellis-policy`, the structure and rule-change checker. |
| [`docs/style.md`](docs/style.md) | The style guide the lints and the checker enforce. |

[CONTRIBUTING.md](CONTRIBUTING.md) covers working on trellis itself.

## License

[MIT](LICENSE).
