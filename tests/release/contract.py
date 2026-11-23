"""Checks the update contract a generated public binary's releases keep.

Usage: contract.py <project> <file name>...

docs/stability.md promises a public binary's users, and the updaters of its installed copies,
that releases are tagged `vX.Y.Z` and that `attestation.json` is one Sigstore bundle whose
statement names every release file by its name and SHA-256. This checks both in <project>:

- The tag: `release-plz.toml` names the main package's tag `v{{ version }}` and only that package
  makes one, and the release workflow reads the tag back in that form.
- The attestation: the publish job of `.github/workflows/release.yml` downloads every target's
  files into `dist`, then at once writes the checksums of every file there, then only attests that
  checksums file, as each attempt's only subject, with plain `sleep`s between the attempts, then at
  once publishes the bundle of the attempt that worked as `dist/attestation.json` beside the files,
  as its last step. No other step can touch `dist` between the download and the upload. It then runs
  the job's own checksums step on stand-ins for the <file name>s and checks each line names one
  file with a SHA-256, every file once.

It prints each break and exits with 1 when there is one.
"""

import hashlib
import os
import re
import subprocess
import sys
import tempfile
import tomllib

import yaml

project, *files = sys.argv[1:]
errors = []


def uses(step, action):
    """Whether `step` runs `action`, at any version."""
    return str(step.get("uses", "")).startswith(action + "@")


def named(name):
    """The publish job's steps named `name`."""
    return [step for step in steps if step.get("name") == name]


with open(os.path.join(project, "release-plz.toml"), "rb") as file:
    release_plz = tomllib.load(file)
with open(os.path.join(project, ".github/workflows/release.yml"), encoding="utf-8") as file:
    jobs = yaml.safe_load(file)["jobs"]
main = release_plz["package"][0]["name"] if release_plz.get("package") else None
workspace = release_plz.get("workspace", {})
if workspace.get("git_tag_name") != "v{{ version }}":
    errors.append(f"release-plz.toml's [workspace] must name the tag v{{{{ version }}}}: {workspace.get('git_tag_name')}")
for package in release_plz.get("package", []):
    tag_name = package.get("git_tag_name", workspace.get("git_tag_name"))
    tags = package.get("git_tag_enable", workspace.get("git_tag_enable", True))
    if package["name"] == main and (not tags or tag_name != "v{{ version }}"):
        errors.append(f"release-plz.toml's main package, {main}, must make the tag v{{{{ version }}}}: {tag_name}, enabled {tags}")
    elif package["name"] != main and tags:
        errors.append(f"release-plz.toml lets {package['name']} make a tag, which only {main} makes")
if workspace.get("git_tag_enable", True):
    errors.append("release-plz.toml's [workspace] must leave tags to the main package, with git_tag_enable = false")
release_steps = jobs["release"]["steps"]
scripts = "\n".join(str(step.get("run", "")) for step in release_steps)
if f'select(.package_name == "{main}") | .tag' not in scripts or "git tag --points-at HEAD --list 'v*'" not in scripts:
    errors.append(f"the release job must read the tag of {main}, or a v* tag on HEAD")
if "--match 'v[0-9]*'" not in scripts and any("release-plz" in str(step.get("uses", "")) for step in release_steps):
    errors.append("the release job must find the last release by a v[0-9]* tag")

steps = jobs["publish"]["steps"]
downloads = [step for step in steps if uses(step, "actions/download-artifact")]
checksums_steps = named("Write the checksums")
attests = [step for step in steps if uses(step, "actions/attest-build-provenance")]
publishes = named("Publish the release")
checksums = None
if len(downloads) != 1 or downloads[0].get("with") != {"path": "dist", "merge-multiple": True}:
    errors.append("the job must download every target's files into dist with one download-artifact step")
if len(checksums_steps) != 1:
    errors.append("the job must have one step named Write the checksums")
else:
    step = checksums_steps[0]
    match = re.fullmatch(r"sha256sum -- \* > ([A-Za-z0-9._-]+)", str(step.get("run", "")).strip())
    if step.get("working-directory") != "dist" or not match:
        errors.append("Write the checksums must run `sha256sum -- * > <file>` in dist, over every file there")
    else:
        checksums = match.group(1)
    if len(downloads) == 1 and steps.index(step) != steps.index(downloads[0]) + 1:
        errors.append("Write the checksums must come right after the download, so nothing changes dist first")
if len(attests) != 3:
    errors.append(f"the job must try attesting three times, and has {len(attests)} attest steps")
for step in attests:
    if checksums and step.get("with") != {"subject-checksums": f"dist/{checksums}"}:
        errors.append(f"{step.get('name')} must take dist/{checksums} as its only input, so its one statement names every file: {step.get('with')}")
if checksums_steps and attests:
    first, last = steps.index(checksums_steps[0]), steps.index(attests[-1])
    if steps.index(attests[0]) != first + 1:
        errors.append("the first attest attempt must come right after Write the checksums")
    for step in steps[first + 1 : last]:
        if step in attests:
            continue
        if not re.fullmatch(r"sleep [0-9]+", str(step.get("run", "")).strip()) or set(step) - {"name", "if", "run"}:
            errors.append(f"only the attest attempts and plain `sleep N` waits may come between Write the checksums and the last attempt: {step.get('name', step.get('uses'))}")
if len(publishes) != 1:
    errors.append("the job must have one step named Publish the release")
else:
    step = publishes[0]
    script = str(step.get("run", ""))
    bundle = str(step.get("env", {}).get("BUNDLE", ""))
    ids = [attest.get("id") for attest in attests]
    if attests and steps.index(step) != steps.index(attests[-1]) + 1:
        errors.append("Publish the release must come right after the last attest attempt, so nothing changes dist after the attestation")
    if steps.index(step) != len(steps) - 1:
        errors.append("Publish the release must be the job's last step")
    if script.count("attestation.json") != 1 or 'cp "$BUNDLE" dist/attestation.json' not in script:
        errors.append("Publish the release must copy one bundle, $BUNDLE, to dist/attestation.json")
    if not ids or any(f"steps.{name}.outputs.bundle-path" not in bundle for name in ids):
        errors.append(f"BUNDLE must be the bundle-path of the attest attempt that worked: {bundle}")
    if script.count("dist/*") != 2 or re.search(r"\b(mv|rm|sed|tee|truncate|ln)\b|>", script.replace("> /dev/null 2>&1", "")):
        errors.append("Publish the release must upload dist/* as it is, every file with the attestation, both when it creates the release and when it resumes one")
if checksums:
    with tempfile.TemporaryDirectory() as dist:
        for name in files:
            with open(os.path.join(dist, name), "w", encoding="utf-8") as stand_in:
                stand_in.write(name)
        subprocess.run(["bash", "-eo", "pipefail", "-c", checksums_steps[0]["run"]], cwd=dist, check=True)
        with open(os.path.join(dist, checksums), encoding="utf-8") as listed:
            lines = listed.read().splitlines()
    listed_files = {}
    for line in lines:
        match = re.fullmatch(r"([0-9a-f]{64})  (\S+)", line)
        if not match:
            errors.append(f"{checksums} has a line without a SHA-256 and a name: {line}")
            continue
        listed_files[match.group(2)] = match.group(1)
    expected = {name: hashlib.sha256(name.encode()).hexdigest() for name in files}
    if listed_files != expected:
        errors.append(f"{checksums} must name each release file once with its SHA-256: {sorted(listed_files)} for {sorted(expected)}")
for error in errors:
    print(f"error: update contract: {project}: {error}", file=sys.stderr)
sys.exit(1 if errors else 0)
