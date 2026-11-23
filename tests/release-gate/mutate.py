"""Breaks the release gate in a copy of a generated project, one way at a time.

Usage: mutate.py <project> <mutation>
       mutate.py --list <project>

`just release-gate` runs gate.py on each mutation `--list` names for <project>, which must fail
on every one. Each mutation rewrites `.github/workflows/ci.yml`, `.github/workflows/release.yml`
or `release-plz.toml` in <project> in place.
"""

import pathlib
import sys

import yaml

STRICT = "just policy-release"


def edit(name, change):
    """Applies `change` to the workflow `name`, parsed, and writes it back."""
    path = pathlib.Path(project) / ".github" / "workflows" / name
    with open(path, encoding="utf-8") as file:
        document = yaml.safe_load(file)
    change(document)
    with open(path, "w", encoding="utf-8") as file:
        yaml.safe_dump(document, file, sort_keys=False, width=1000)


def ci_steps(document):
    """The quick job's steps."""
    return document["jobs"]["quick"]["steps"]


def release_steps(document):
    """The release job's steps."""
    return document["jobs"]["release"]["steps"]


def strict(steps):
    """The step that runs the strict check."""
    return next(step for step in steps if step.get("run") == STRICT)


def find_step(steps):
    """The step that finds whether this push releases."""
    return next(step for step in steps if step.get("id") == "releases")


def move_after_publish(document):
    """Moves the strict check after the release-plz step that tags and publishes."""
    steps = release_steps(document)
    step = strict(steps)
    steps.remove(step)
    publish = next(
        index
        for index, other in enumerate(steps)
        if str(other.get("uses", "")).startswith("release-plz/action@")
    )
    steps.insert(publish + 1, step)


def drop_last_needs(document):
    """Lets the last job start without waiting for the release job."""
    last = list(document["jobs"].values())[-1]
    last.pop("needs", None)


def install_step(steps):
    """The step that installs just in the release job."""
    return next(
        step
        for step in steps
        if str(step.get("uses", "")).startswith("taiki-e/install-action@")
        and "just@" in str(step.get("with", {}).get("tool", ""))
    )


def move_install_after_strict(document):
    """Moves the step that installs just after the strict check."""
    steps = release_steps(document)
    step = install_step(steps)
    steps.remove(step)
    steps.insert(steps.index(strict(steps)) + 1, step)


def drop_pull_request(document):
    """Stops CI from running on pull requests."""
    triggers = document.get(True) or document.get("on")
    triggers.pop("pull_request")


def edit_release_plz(old, new):
    """A mutation that replaces `old` with `new` in release-plz.toml."""

    def change(path):
        text = path.read_text(encoding="utf-8")
        assert old in text, f"release-plz.toml has no {old!r}"
        path.write_text(text.replace(old, new, 1), encoding="utf-8")

    return change


def replace_in_find(old, new):
    """A mutation that replaces `old` with `new` in the finding step's script."""

    def change(document):
        step = find_step(release_steps(document))
        assert old in step["run"], f"the finding step's script has no {old!r}"
        step["run"] = step["run"].replace(old, new)

    return change


MUTATIONS = {
    "ci-strict-step-removed": lambda document: ci_steps(document).remove(strict(ci_steps(document))),
    "ci-strict-step-on-every-pull-request": lambda document: strict(ci_steps(document)).pop("if"),
    "ci-strict-step-on-other-branches": lambda document: strict(ci_steps(document)).update(
        {"if": "startsWith(github.head_ref, 'release-')"}
    ),
    "release-strict-step-removed": lambda document: release_steps(document).remove(
        strict(release_steps(document))
    ),
    "release-strict-step-after-publish": move_after_publish,
    "release-strict-step-never-runs": lambda document: strict(release_steps(document)).update(
        {"if": "steps.releases.outputs.releases == 'false'"}
    ),
    "release-strict-step-unconditional": lambda document: strict(release_steps(document)).pop("if"),
    "ci-strict-step-continues-on-error": lambda document: strict(ci_steps(document)).update(
        {"continue-on-error": True}
    ),
    "release-strict-step-continues-on-error": lambda document: strict(
        release_steps(document)
    ).update({"continue-on-error": True}),
    "release-find-step-unanchored": replace_in_find("'^release-plz-'", "'release-plz-'"),
    "release-find-step-fails-open": replace_in_find(
        "--jq '.[].head.ref')", "--jq '.[].head.ref' || true)"
    ),
    "release-find-step-other-commit": replace_in_find("$GITHUB_SHA", "HEAD"),
    "release-job-not-needed": drop_last_needs,
    "release-find-step-ignores-tags": replace_in_find(
        " || [ -n \"$(git tag --points-at HEAD --list 'v*')\" ]", ""
    ),
    "release-install-step-removed": lambda document: release_steps(document).remove(
        install_step(release_steps(document))
    ),
    "release-install-step-after-strict": move_install_after_strict,
    "release-install-step-on-another-condition": lambda document: install_step(
        release_steps(document)
    ).update({"if": "steps.releases.outputs.releases == 'false'"}),
    "ci-pull-requests-dropped": drop_pull_request,
    "release-find-step-renamed": lambda document: find_step(release_steps(document)).update(
        {"name": "Find the release"}
    ),
}

RELEASE_PLZ_MUTATIONS = {
    "release-plz-other-prefix": edit_release_plz(
        "release_always = false\n", "release_always = false\npr_branch_prefix = \"release-\"\n"
    ),
    "release-plz-releases-always": edit_release_plz(
        "release_always = false\n", "release_always = true\n"
    ),
}

TAG_MUTATIONS = {"release-find-step-ignores-tags"}


def reads_tag(path):
    """Whether the release job of the project at `path` reads a tag on the commit."""
    workflow = pathlib.Path(path) / ".github" / "workflows" / "release.yml"
    with open(workflow, encoding="utf-8") as file:
        steps = yaml.safe_load(file)["jobs"]["release"]["steps"]
    return any(step.get("id") == "tag" for step in steps)


if sys.argv[1:2] == ["--list"]:
    tagged = reads_tag(sys.argv[2])
    names = [name for name in MUTATIONS if tagged or name not in TAG_MUTATIONS]
    print("\n".join([*names, *RELEASE_PLZ_MUTATIONS]))
    sys.exit(0)
project, mutation = sys.argv[1:]
if mutation in RELEASE_PLZ_MUTATIONS:
    RELEASE_PLZ_MUTATIONS[mutation](pathlib.Path(project) / "release-plz.toml")
else:
    workflow = "ci.yml" if mutation.startswith("ci-") else "release.yml"
    edit(workflow, MUTATIONS[mutation])
