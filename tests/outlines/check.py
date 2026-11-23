"""Checks the README outline, and the screenshots page and rules, of a project trellis generated.

Usage: check.py <project> <interfaces>

`interfaces` is the project's answer, comma-separated, such as `web,cli`. The README must hold a
`**To write:**` prompt in each part the owner writes: the opening, the features and the picture or
sample before `## Quick start`, and one under it. Each prompt must be shaped by the interfaces,
and the outline must reference no image, since none exists yet. A project with a web, desktop or
terminal interface must get `docs/usage.md` with its screenshots prompts, the README's index entry
for it, CONTRIBUTING.md's Screenshots rules and the pull request template's line about images; any
other must get none of them.
"""

import pathlib
import re
import sys

PROMPT = "> **To write:**"
UI = ("web", "desktop", "terminal")
# A phrase only the prompt for each primary interface holds, as the picture or sample part writes it.
PRIMARY = {
    "web": "at a desktop width",
    "desktop": "at its default size",
    "terminal": "an SVG capture of the screen",
    "cli": "the one command that best shows the main promise",
    "service": "what it produces",
    "library": "the smallest example that does the main thing",
}
# A phrase only the Quick start prompt for each secondary interface holds.
SECONDARY = {
    "web": "> - its web app: where its UI listens",
    "desktop": "> - its desktop app: how to start it",
    "terminal": "> - its terminal app: how to start it",
    "cli": "> - its command-line tool: the command a person runs first",
    "service": "> - its service: how to start it in the background",
}
# A phrase only the screenshots prompt for each UI interface holds.
GALLERY = {
    "web": "390×844 CSS pixels at 2×",
    "desktop": "Capture each window at its default size.",
    "terminal": "screenshots/<screen>.svg",
}

project = pathlib.Path(sys.argv[1])
interfaces = sys.argv[2].split(",")
failures = []


def check(condition, message):
    if not condition:
        failures.append(message)


def images(text):
    """The local image references in `text` outside code spans and fenced blocks. A badge is an
    image too, but from a URL."""
    text = re.sub(r"(?ms)^> ?```.*?^> ?```", "", text)
    text = re.sub(r"`[^`\n]*`", "", text)
    return re.findall(r"!\[[^\]]*\]\((?!https://)|<img\b(?![^>]*src=\"https://)", text)


def long_lines(path):
    """The lines of the Markdown file `path` over 100 characters, outside tables, code blocks and
    badges, as `path:line`."""
    found = []
    fenced = False
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        bare = line.lstrip("> ")
        if bare.startswith("```"):
            fenced = not fenced
            continue
        if fenced or bare.startswith("|") or bare.startswith("[![") or len(line) <= 100:
            continue
        found.append(f"{path.relative_to(project)}:{number}")
    return found


manifest = (project / "Cargo.toml").read_text(encoding="utf-8")
listed = re.search(r"^interfaces = (\[.*\])$", manifest, re.M)
check(
    listed and listed.group(1) == "[" + ", ".join(f'"{name}"' for name in interfaces) + "]",
    f"Cargo.toml's interfaces line isn't {interfaces}, in that order",
)
markdown = [project / "README.md", project / "CONTRIBUTING.md", *sorted((project / "docs").glob("*.md"))]
for path in markdown:
    for where in long_lines(path):
        check(False, f"{where} runs over 100 characters")

readme = (project / "README.md").read_text(encoding="utf-8")
head, _, rest = readme.partition("\n## Quick start\n")
quick_start, _, rest = rest.partition("\n## Documentation\n")
index, _, _ = rest.partition("\n## ")
check(head.count(PROMPT) == 3, f"the README has {head.count(PROMPT)} prompts before Quick start, not 3")
check(quick_start.count(PROMPT) == 1, "the README's Quick start has no prompt")
check(PROMPT not in index, "the README's Documentation section holds a prompt")
check(PRIMARY[interfaces[0]] in head, f"the README's picture or sample isn't the one for {interfaces[0]}")
for name, phrase in PRIMARY.items():
    if name != interfaces[0]:
        check(phrase not in readme, f"the README holds the prompt for a primary {name}")
for name, phrase in SECONDARY.items():
    check((phrase in quick_start) == (name in interfaces[1:]), f"the README's Quick start prompt for a secondary {name} is wrong")
check(not images(readme), "the README references an image")
check(
    re.search(r'^readme_sections = \["Quick start", "Documentation"(, "License")?\]$', manifest, re.M),
    "readme_sections isn't Quick start, Documentation and License",
)
check("(docs/style.md)" not in index and "(docs/maintaining.md)" not in index, "the README indexes a contributors' page")
contributing = (project / "CONTRIBUTING.md").read_text(encoding="utf-8")
check(
    "- [Code style](docs/style.md):" in contributing and "- [Maintain](docs/maintaining.md):" in contributing,
    "CONTRIBUTING.md doesn't index the contributors' pages",
)

ui = [name for name in interfaces if name in UI]
usage = project / "docs" / "usage.md"
template = (project / ".github" / "pull_request_template.md").read_text(encoding="utf-8")
check(usage.exists() == bool(ui), "docs/usage.md exists" if usage.exists() else "docs/usage.md is missing")
check(("(docs/usage.md)" in index) == bool(ui), "the README's index entry for docs/usage.md is wrong")
check(("\n### Screenshots\n" in contributing) == bool(ui), "CONTRIBUTING.md's Screenshots rules are wrong")
check(("the privacy check ran" in template) == bool(ui), "the pull request template's line about images is wrong")
if usage.exists():
    gallery = usage.read_text(encoding="utf-8")
    above, _, screenshots = gallery.partition("\n## Screenshots\n")
    check(re.match(r"# Use demo\n\n[A-Z]", gallery), "docs/usage.md doesn't open with its title and a paragraph")
    check(above.count(PROMPT) == len(ui), "docs/usage.md lacks a prompt for using each UI")
    check(screenshots.count(PROMPT) == len(ui), "docs/usage.md's Screenshots lacks a prompt for each UI")
    check("invented" in screenshots, "docs/usage.md's Screenshots prompt doesn't ask to say the data is invented")
    for name, phrase in GALLERY.items():
        check((phrase in screenshots) == (name in ui), f"docs/usage.md's Screenshots prompt for {name} is wrong")
    check(not images(gallery), "docs/usage.md references an image")
    check(("390×844" in contributing) == ("web" in ui), "CONTRIBUTING.md's web sizes are wrong")
    check(("<desc>" in contributing) == ("terminal" in ui), "CONTRIBUTING.md's terminal captures are wrong")

for failure in failures:
    print(f"error: {project.name} {interfaces}: {failure}", file=sys.stderr)
sys.exit(1 if failures else 0)
