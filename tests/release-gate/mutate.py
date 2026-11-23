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

STRICT_STEP = "Check the docs as a release does"
AFTER_STEP = "Check the docs as a release does, after release-plz released"


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
    """The step that runs the strict check, before release-plz in the release job."""
    return next(step for step in steps if step.get("name") == STRICT_STEP)


def after_step(steps):
    """The step that runs the strict check after release-plz."""
    return next(step for step in steps if step.get("name") == AFTER_STEP)


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


def just_installs(steps):
    """The steps that install just, in order."""
    return [
        step
        for step in steps
        if str(step.get("uses", "")).startswith("taiki-e/install-action@")
        and "just@" in str(step.get("with", {}).get("tool", ""))
    ]


def install_step(steps):
    """The step that installs just for the strict check before release-plz."""
    return just_installs(steps)[0]


def after_install_step(steps):
    """The step that installs just for the strict check after release-plz."""
    return just_installs(steps)[-1]


def move_after_check_before_release_plz(document):
    """Moves the check after release-plz, and its install, before the release-plz step."""
    steps = release_steps(document)
    moved = [after_install_step(steps), after_step(steps)]
    for step in moved:
        steps.remove(step)
    publish = next(
        index
        for index, other in enumerate(steps)
        if str(other.get("uses", "")).startswith("release-plz/action@")
    )
    steps[publish:publish] = moved


def ignore_release_failure(document):
    """Lets the first job after the release job run even when the release job fails: the build or
    image job of a binary or service, or a library's release-pr job."""
    job = list(document["jobs"].values())[1]
    job["if"] = f"always() && ({job['if']})" if "if" in job else "always()"


def invert_after_check(document):
    """Runs the check after release-plz only when the check before it ran."""
    step = after_step(release_steps(document))
    step["if"] = step["if"].replace("!= 'true'", "== 'true'", 1)


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
    "release-find-step-ignores-subject": replace_in_find(
        "number=$(git log -1 --format=%s | sed -nE 's/.* \\(#([0-9]+)\\)$/\\1/p')", "number="
    ),
    "release-find-step-pull-lookup-fails-open": replace_in_find(
        'cat "$error" >&2\n    exit 1', 'cat "$error" >&2\n    merged=false'
    ),
    "release-find-step-not-found-fatal": replace_in_find(
        "then\n    merged=false\n  else", "then\n    exit 1\n  else"
    ),
    "release-find-step-trusts-the-subject": replace_in_find(
        '[ "$merged" = true ] && [ "$merge_sha" = "$GITHUB_SHA" ]', "true"
    ),
    "release-find-step-ignores-the-merge-commit": replace_in_find(
        ' && [ "$merge_sha" = "$GITHUB_SHA" ]', ""
    ),
    "release-find-step-ignores-merged": replace_in_find('[ "$merged" = true ] && ', ""),
    "release-find-step-first-number": replace_in_find(
        "s/.* \\(#([0-9]+)\\)$/\\1/p", "s/^[^(]* \\(#([0-9]+)\\).*/\\1/p"
    ),
    "release-find-step-other-pull": replace_in_find("pulls/$number", "pulls/1"),
    "release-job-failure-ignored": ignore_release_failure,
    "release-find-step-skips-direct-pushes": replace_in_find(
        ' || { [ -z "$number" ] && [ "$CREATED" != true ]; }', ""
    ),
    "release-find-step-checks-the-creating-push": replace_in_find(' && [ "$CREATED" != true ]', ""),
    "release-find-step-created-dropped": lambda document: find_step(release_steps(document))[
        "env"
    ].pop("CREATED"),
    "release-find-step-created-rebound": lambda document: find_step(release_steps(document))[
        "env"
    ].update({"CREATED": "${{ github.event.forced }}"}),
    "release-after-check-removed": lambda document: release_steps(document).remove(
        after_step(release_steps(document))
    ),
    "release-after-check-inverted": invert_after_check,
    "release-after-check-continues-on-error": lambda document: after_step(
        release_steps(document)
    ).update({"continue-on-error": True}),
    "release-after-check-before-release-plz": move_after_check_before_release_plz,
    "release-after-install-removed": lambda document: release_steps(document).remove(
        after_install_step(release_steps(document))
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

TAG_MUTATIONS = {
    "release-find-step-ignores-tags",
    "release-after-check-removed",
    "release-after-check-inverted",
    "release-after-check-continues-on-error",
    "release-after-check-before-release-plz",
    "release-after-install-removed",
}

LIBRARY_MUTATIONS = {
    "release-find-step-skips-direct-pushes",
    "release-find-step-checks-the-creating-push",
    "release-find-step-created-dropped",
    "release-find-step-created-rebound",
}


def reads_tag(path):
    """Whether the release job of the project at `path` reads a tag on the commit."""
    workflow = pathlib.Path(path) / ".github" / "workflows" / "release.yml"
    with open(workflow, encoding="utf-8") as file:
        steps = yaml.safe_load(file)["jobs"]["release"]["steps"]
    return any(step.get("id") == "tag" for step in steps)


if sys.argv[1:2] == ["--list"]:
    tagged = reads_tag(sys.argv[2])
    names = [
        name
        for name in MUTATIONS
        if (tagged or name not in TAG_MUTATIONS) and (not tagged or name not in LIBRARY_MUTATIONS)
    ]
    print("\n".join([*names, *RELEASE_PLZ_MUTATIONS]))
    sys.exit(0)
project, mutation = sys.argv[1:]
if mutation in RELEASE_PLZ_MUTATIONS:
    RELEASE_PLZ_MUTATIONS[mutation](pathlib.Path(project) / "release-plz.toml")
else:
    workflow = "ci.yml" if mutation.startswith("ci-") else "release.yml"
    edit(workflow, MUTATIONS[mutation])
