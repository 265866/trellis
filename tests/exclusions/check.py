"""Renders every `_exclude` entry of copier.yml with the answers a project generated before the
newer questions has, and fails if an entry can't render.

Usage: check.py <copier.yml>

Copier matches a new trellis version's exclusions against the files of the version a project
updates from, with that version's answers, so an entry that reads an answer the older version
didn't ask fails the update. `ANSWERED` lists the answers every trellis version since 0.15.0 asks;
an entry must read any other with a default.
"""

import itertools
import sys

import jinja2
import yaml

ANSWERED = {
    "kind": ["binary", "service", "library"],
    "visibility": ["public", "private"],
    "license": ["MIT", "MIT OR Apache-2.0", "none"],
    "publish_crate": [False, True],
    "platforms": [["linux", "macos", "windows"]],
    "_copier_operation": ["copy", "update"],
}

with open(sys.argv[1], encoding="utf-8") as file:
    config = yaml.safe_load(file)
environment = jinja2.Environment(undefined=jinja2.StrictUndefined)
failures = []
for entry in config["_exclude"]:
    template = environment.from_string(entry)
    for values in itertools.product(*ANSWERED.values()):
        answers = dict(zip(ANSWERED, values))
        answers.update(project_name="demo", crate_name="demo")
        try:
            template.render(**answers)
        except jinja2.UndefinedError as error:
            failures.append(f"{entry}: {error}, with {answers}")
            break
for failure in failures:
    print(f"error: copier.yml's _exclude entry {failure}", file=sys.stderr)
sys.exit(1 if failures else 0)
