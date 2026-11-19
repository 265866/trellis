# AGENTS.md

trellis is a Copier template for new Rust projects, plus the custom lints and the structure checker
those projects load. [CONTRIBUTING.md](CONTRIBUTING.md) has the setup, the checks and the
procedures. This file adds what an agent needs on top.

## Layout

| Path | Holds |
| --- | --- |
| `template/` | The files a generated project gets, most of them Jinja. [copier.yml](copier.yml) has the questions. |
| `lints/` | The custom lints, a dylint library that builds on its own nightly. |
| `policy/` | `trellis-policy`, the structure and rule-change checker. |
| `tests/dependencies/` | The scripts `just dependencies` runs: the crates most projects add, then code the lints must reject. |

## Fix the template, not the output

A generated project is output. When one of its checks fails, fix `template/`, `lints/` or
`policy/`, never the project. `just template <kind>` generates a `binary`, `service` or `library`
from the working tree and runs its `just ci`. `just templates` generates every kind and answer set
and runs their checks; it is the proof that a template change works. In a `.jinja` file, text the
project needs as written, such as the justfile's `{{ ... }}` or a workflow's `${{ ... }}`, goes
inside `{% raw %}`.

## Copies that change together

- `docs/style.md` is a copy of `template/docs/style.md`, the generated projects' style guide. It
  isn't trellis's own rules. Change both.
- The config files and workflows that the `pins` recipe in the [justfile](justfile) compares with
  their copies in `template/`, and the `[workspace.lints]` tables of `Cargo.toml` and
  `template/Cargo.toml.jinja`.
- Every pinned version, as "Update pinned versions" in [CONTRIBUTING.md](CONTRIBUTING.md) lists.

`just pins` fails until each copy agrees.

## Rules from the generated projects

The Code section of a generated project's rules, in
[template/CONTRIBUTING.md.jinja](template/CONTRIBUTING.md.jinja), applies here too, with its
Comments and Silencing a lint. The rest describe a generated project: its `crates/`,
`just policy`, release-plz and its docs pages. A rule change here follows "Change a rule" in
CONTRIBUTING.md.

## Before pushing

Run `just ci` and fix what fails. Never weaken a check to get it passing.
