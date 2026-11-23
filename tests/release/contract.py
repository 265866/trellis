"""Checks that a generated binary's release attests its files as one bundle that names each.

Usage: attestation.py <release.yml> <file name>...

docs/stability.md promises a public binary's users, and the updaters of installed copies, that
`attestation.json` is one Sigstore bundle whose statement names every release file by its name and
SHA-256. The publish job of <release.yml> must keep that shape: it downloads every target's files
into `dist`, writes the checksums of every file there, attests that checksums file alone as each
attempt's only subject, and publishes the bundle of the attempt that worked as
`dist/attestation.json` beside the files. It then runs the job's own checksums step on stand-ins
for the <file name>s and checks each line names one file with a SHA-256, every file once. It
prints each break and exits with 1 when there is one.
"""

import hashlib
import os
import re
import subprocess
import sys
import tempfile

import yaml

workflow, *files = sys.argv[1:]
errors = []
with open(workflow, encoding="utf-8") as file:
    steps = yaml.safe_load(file)["jobs"]["publish"]["steps"]


def uses(step, action):
    """Whether `step` runs `action`, at any version."""
    return str(step.get("uses", "")).startswith(action + "@")


downloads = [step for step in steps if uses(step, "actions/download-artifact")]
if len(downloads) != 1 or downloads[0].get("with") != {"path": "dist", "merge-multiple": True}:
    errors.append("the job must download every target's files into dist with one download-artifact step")
checksums_steps = [step for step in steps if step.get("name") == "Write the checksums"]
checksums = None
if len(checksums_steps) != 1:
    errors.append("the job must have one step named Write the checksums")
else:
    step = checksums_steps[0]
    match = re.fullmatch(r"sha256sum -- \* > ([A-Za-z0-9._-]+)", str(step.get("run", "")).strip())
    if step.get("working-directory") != "dist" or not match:
        errors.append("Write the checksums must run `sha256sum -- * > <file>` in dist, over every file there")
    else:
        checksums = match.group(1)
attests = [step for step in steps if uses(step, "actions/attest-build-provenance")]
if len(attests) != 3:
    errors.append(f"the job must try attesting three times, and has {len(attests)} attest steps")
for step in attests:
    if checksums and step.get("with") != {"subject-checksums": f"dist/{checksums}"}:
        errors.append(f"{step.get('name')} must take dist/{checksums} as its only input, so its one statement names every file: {step.get('with')}")
if checksums_steps and attests:
    first, last = steps.index(checksums_steps[0]), steps.index(attests[-1])
    between = [step.get("name", step.get("uses")) for step in steps[first + 1 : last] if step not in attests]
    if steps.index(attests[0]) < first or any(not str(name).startswith("Wait ") for name in between):
        errors.append(f"only the attest steps and their waits may come between Write the checksums and the last attempt: {between}")
publishes = [step for step in steps if step.get("name") == "Publish the release"]
if len(publishes) != 1:
    errors.append("the job must have one step named Publish the release")
else:
    step = publishes[0]
    script = str(step.get("run", ""))
    bundle = str(step.get("env", {}).get("BUNDLE", ""))
    ids = [attest.get("id") for attest in attests]
    if script.count("attestation.json") != 1 or 'cp "$BUNDLE" dist/attestation.json' not in script:
        errors.append("Publish the release must copy one bundle, $BUNDLE, to dist/attestation.json")
    if not ids or any(f"steps.{name}.outputs.bundle-path" not in bundle for name in ids):
        errors.append(f"BUNDLE must be the bundle-path of the attest attempt that worked: {bundle}")
    if script.count("dist/*") != 2:
        errors.append("Publish the release must upload dist/*, every file with the attestation, both when it creates the release and when it resumes one")
if checksums:
    with tempfile.TemporaryDirectory() as dist:
        for name in files:
            with open(os.path.join(dist, name), "w", encoding="utf-8") as stand_in:
                stand_in.write(name)
        subprocess.run(["bash", "-eo", "pipefail", "-c", checksums_steps[0]["run"]], cwd=dist, check=True)
        with open(os.path.join(dist, checksums), encoding="utf-8") as listed:
            lines = listed.read().splitlines()
    named = {}
    for line in lines:
        match = re.fullmatch(r"([0-9a-f]{64})  (\S+)", line)
        if not match:
            errors.append(f"{checksums} has a line without a SHA-256 and a name: {line}")
            continue
        named[match.group(2)] = match.group(1)
    expected = {name: hashlib.sha256(name.encode()).hexdigest() for name in files}
    if named != expected:
        errors.append(f"{checksums} must name each release file once with its SHA-256: {sorted(named)} for {sorted(expected)}")
for error in errors:
    print(f"error: attestation: {workflow}: {error}", file=sys.stderr)
sys.exit(1 if errors else 0)
