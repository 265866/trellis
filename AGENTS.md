# AGENTS.md

trellis is a Copier template for new Rust projects, plus the lints (`lints/`) and the checker
(`policy/`) those projects load. Follow [CONTRIBUTING.md](CONTRIBUTING.md), and the rules a
generated project's `AGENTS.md` sets, which
[template/AGENTS.md.jinja](template/AGENTS.md.jinja) holds.

Run `just ci` before every commit. `just templates` is the proof that a template change works:
it generates every kind of project and runs its checks.
