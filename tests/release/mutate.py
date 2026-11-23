"""Breaks the update contract in a copy of a generated public binary, one way at a time.

Usage: mutate.py <project> <mutation>
       mutate.py --list

`just release-extras` runs contract.py on each mutation, which must fail on every one. Each
mutation rewrites `release-plz.toml` or `.github/workflows/release.yml` in <project> in place.
"""

import sys

import yaml


def load():
    """The workflow, parsed."""
    with open(workflow, encoding="utf-8") as file:
        return yaml.safe_load(file)


def save(document):
    """Writes `document` back as the workflow."""
    with open(workflow, "w", encoding="utf-8") as file:
        yaml.safe_dump(document, file, sort_keys=False, width=1000)


def edit_steps(change):
    """Applies `change` to the publish job's steps."""
    document = load()
    change(document["jobs"]["publish"]["steps"])
    save(document)


def checksums(steps):
    """The checksums file the job writes."""
    step = next(step for step in steps if step.get("name") == "Write the checksums")
    return step["run"].split("> ")[-1].strip()


def attests(steps):
    """The attest attempts among `steps`."""
    return [step for step in steps if str(step.get("uses", "")).startswith("actions/attest-build-provenance@")]


def after_last_attest(new):
    """A change that adds the step `new` right after the last attest attempt."""
    def change(steps):
        steps.insert(steps.index(attests(steps)[-1]) + 1, new)

    return change


def subject_path(steps):
    """Attests every file by its path."""
    for step in attests(steps):
        step["with"] = {"subject-path": "dist/*"}


def one_file_first(steps):
    """Attests one file in the first attempt."""
    attests(steps)[0]["with"] = {"subject-path": "dist/demo-binary-0.1.0-linux.tar.gz"}


def checksums_without_digests(steps):
    """Lists the names without digests."""
    step = next(step for step in steps if step.get("name") == "Write the checksums")
    step["run"] = f"ls > {checksums(steps)}"


def checksums_of_some(steps):
    """Lists the archives alone."""
    step = next(step for step in steps if step.get("name") == "Write the checksums")
    step["run"] = f"sha256sum -- *.tar.gz > {checksums(steps)}"


def bundle_per_file(steps):
    """Publishes the bundle once per file."""
    step = next(step for step in steps if step.get("name") == "Publish the release")
    step["run"] = step["run"].replace(
        'cp "$BUNDLE" dist/attestation.json', 'for file in dist/*; do cp "$BUNDLE" "$file.json"; done'
    )


def delete_before_checksums(steps):
    """Deletes files before the checksums."""
    download = next(step for step in steps if str(step.get("uses", "")).startswith("actions/download-artifact@"))
    steps.insert(steps.index(download) + 1, {"name": "Tidy", "run": "rm -f dist/*.zip"})


def rewriting_wait(steps):
    """Adds a step named Wait that drops a checksum."""
    first = steps.index(attests(steps)[0])
    steps.insert(first + 1, {"name": "Wait a moment", "run": f"sed -i 1d dist/{checksums(steps)}"})


def edit_text(path, old, new):
    """Replaces `old` with `new` in the file at `path`, which must hold it."""
    with open(path, encoding="utf-8") as file:
        text = file.read()
    if old not in text:
        sys.exit(f"error: {path} has no {old}")
    with open(path, "w", encoding="utf-8") as file:
        file.write(text.replace(old, new))


MUTATIONS = {
    "attest every file by path, not the checksums file": lambda: edit_steps(subject_path),
    "attest one file in the first attempt": lambda: edit_steps(one_file_first),
    "write checksums without digests": lambda: edit_steps(checksums_without_digests),
    "write checksums of some files": lambda: edit_steps(checksums_of_some),
    "publish a bundle per file": lambda: edit_steps(bundle_per_file),
    "A: rename a file after the attestation": lambda: edit_steps(
        after_last_attest({"name": "Rename", "run": "mv dist/demo-binary-0.1.0-linux.tar.gz dist/demo-binary-linux.tar.gz"})
    ),
    "A: sign a file again after the attestation": lambda: edit_steps(
        after_last_attest({"name": "Sign", "uses": "sigstore/gh-action-sigstore-python@v3", "with": {"inputs": "dist/*"}})
    ),
    "E: delete files before the checksums": lambda: edit_steps(delete_before_checksums),
    "G: a step named Wait that rewrites the checksums": lambda: edit_steps(rewriting_wait),
    "tag the workspace {{ package }}-v{{ version }}": lambda: edit_text(
        release_plz, 'git_tag_name = "v{{ version }}"', 'git_tag_name = "{{ package }}-v{{ version }}"'
    ),
    "tag the main package another way": lambda: edit_text(
        release_plz, "git_tag_enable = true", 'git_tag_enable = true\ngit_tag_name = "release-{{ version }}"'
    ),
    "read back a tag of any form": lambda: edit_text(
        workflow, "git tag --points-at HEAD --list 'v*'", "git tag --points-at HEAD --list '*'"
    ),
}

if sys.argv[1:] == ["--list"]:
    print("\n".join(MUTATIONS))
    sys.exit(0)
project, mutation = sys.argv[1:]
workflow = f"{project}/.github/workflows/release.yml"
release_plz = f"{project}/release-plz.toml"
MUTATIONS[mutation]()
