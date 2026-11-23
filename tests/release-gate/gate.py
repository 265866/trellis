"""Checks that a generated project can't publish a release while the README is unwritten.

Usage: gate.py <project>

`just policy-release` fails on what `just policy` only notes, such as a README prompt left to
write. This checks in <project> that both release routes run it:

- The release pull request: the quick job of `.github/workflows/ci.yml` runs it on every pull
  request whose head branch starts with `release-plz-`, the branch release-plz opens and the one
  a person makes by hand, and on no other.
- The release itself: the release job of `.github/workflows/release.yml` runs it before the
  release-plz step that tags and publishes, when the step that asks GitHub which pull requests
  merged this push finds one from a `release-plz-` branch, which is when release-plz releases, or,
  in a project whose release job reads a `v*` tag on the commit to build and publish, when the
  commit has one. It installs just before it, and neither strict step may continue on an error.
  Every other job needs the release job, so nothing publishes first. The finding step's own
  script, run as GitHub runs it in a git repository with a stand-in `gh`, finds a merged
  `release-plz-` branch and only that, and the tag where the project reads one, asks for this
  repository's commit, and fails when `gh` does, so an error never skips the check.
- release-plz.toml releases only those pushes: its `pr_branch_prefix` is the default,
  `release-plz-`, and it sets `release_always = false`.

It prints each break and exits with 1 when there is one.
"""

import os
import pathlib
import subprocess
import sys
import tempfile
import tomllib

import yaml

STRICT = "just policy-release"
PR_CONDITION = "github.event_name == 'pull_request' && startsWith(github.head_ref, 'release-plz-')"
FOUND = "steps.releases.outputs.releases == 'true'"
FIND_STEP = "Find whether this push releases"
STEP_SCRIPT = pathlib.Path(__file__).resolve().parent.parent / "release-build" / "step.py"

project = pathlib.Path(sys.argv[1])
workflows = project / ".github" / "workflows"
problems = []


def load(name):
    """The workflow `name`, parsed."""
    with open(workflows / name, encoding="utf-8") as file:
        return yaml.safe_load(file)


def index_of(steps, matches):
    """The index of the only step that `matches`, or None when no step or several do."""
    found = [index for index, step in enumerate(steps) if matches(step)]
    return found[0] if len(found) == 1 else None


def check_pull_request():
    """Checks that CI runs the strict check on release pull requests and only on them."""
    ci = load("ci.yml")
    triggers = ci.get(True) or ci.get("on") or {}
    if "pull_request" not in triggers:
        problems.append("ci.yml doesn't run on pull requests")
    steps = ci["jobs"]["quick"]["steps"]
    strict = index_of(steps, lambda step: step.get("run") == STRICT)
    if strict is None:
        problems.append(f"ci.yml's quick job doesn't run `{STRICT}` in exactly one step")
    elif steps[strict].get("continue-on-error"):
        problems.append(f"ci.yml's `{STRICT}` step continues on an error")
    elif steps[strict].get("if") != PR_CONDITION:
        problems.append(
            f"ci.yml runs `{STRICT}` under `if: {steps[strict].get('if')}`, not on exactly the "
            f"pull requests from a `release-plz-` branch: `if: {PR_CONDITION}`"
        )


def needs(job):
    """The jobs `job` needs, as a list."""
    value = job.get("needs", [])
    return [value] if isinstance(value, str) else list(value)


def check_release():
    """Checks that the release workflow runs the strict check before anything publishes."""
    release = load("release.yml")
    jobs = release["jobs"]
    steps = jobs["release"]["steps"]
    publish = index_of(
        steps,
        lambda step: str(step.get("uses", "")).startswith("release-plz/action@")
        and step.get("with", {}).get("command") == "release",
    )
    find = index_of(steps, lambda step: step.get("id") == "releases")
    strict = index_of(steps, lambda step: step.get("run") == STRICT)
    install = index_of(
        steps,
        lambda step: str(step.get("uses", "")).startswith("taiki-e/install-action@")
        and "just@" in str(step.get("with", {}).get("tool", "")),
    )
    if publish is None:
        problems.append("release.yml's release job has no single release-plz `release` step")
        return
    if find is None or steps[find].get("name") != FIND_STEP:
        problems.append(f"release.yml's release job has no step `{FIND_STEP}` with id releases")
    if strict is None:
        problems.append(f"release.yml's release job doesn't run `{STRICT}` in exactly one step")
        return
    if steps[strict].get("continue-on-error"):
        problems.append(f"release.yml's `{STRICT}` step continues on an error")
    if steps[strict].get("if") != FOUND:
        problems.append(
            f"release.yml runs `{STRICT}` under `if: {steps[strict].get('if')}`, not when a "
            f"release PR merged: `if: {FOUND}`"
        )
    if not (find is not None and find < strict < publish):
        problems.append(
            f"release.yml doesn't find the merged release PR, then run `{STRICT}`, then the "
            "release-plz step that tags and publishes, in that order"
        )
    if install is None or install > strict or steps[install].get("if") not in (None, FOUND):
        problems.append(f"release.yml doesn't install just before `{STRICT}` whenever it runs it")
    for name, job in jobs.items():
        seen, todo = set(), needs(job)
        while todo:
            other = todo.pop()
            if other not in seen:
                seen.add(other)
                todo.extend(needs(jobs.get(other, {})))
        if name != "release" and "release" not in seen:
            problems.append(f"release.yml's {name} job can run before the release job's check")
    if find is not None and steps[find].get("name") == FIND_STEP:
        reads_tag = any(step.get("id") == "tag" for step in steps)
        check_find_step(reads_tag)


def check_release_plz():
    """Checks that release-plz.toml releases what the gate checks: a push that merged a pull request
    from a `release-plz-` branch, and no other."""
    with open(project / "release-plz.toml", "rb") as file:
        workspace = tomllib.load(file).get("workspace", {})
    prefix = workspace.get("pr_branch_prefix", "release-plz-")
    if prefix != "release-plz-":
        problems.append(
            f"release-plz.toml sets `pr_branch_prefix = {prefix!r}`, but the gate checks pull "
            "requests from `release-plz-` branches"
        )
    if workspace.get("release_always") is not False:
        problems.append(
            "release-plz.toml doesn't set `release_always = false`, so release-plz can release a "
            "push the gate doesn't check"
        )


def run_find_step(branches, gh_status=0, tag=None):
    """Runs the finding step's script as GitHub runs it, in a git repository whose commit has
    the tag `tag`, if any, with a `gh` that prints `branches` and exits with `gh_status`, and
    returns its exit code, what it wrote to GITHUB_OUTPUT and the arguments `gh` got."""
    script = subprocess.run(
        [sys.executable, "-I", str(STEP_SCRIPT), str(workflows / "release.yml"), "release", FIND_STEP],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    with tempfile.TemporaryDirectory() as directory:
        directory = pathlib.Path(directory)
        (directory / "branches").write_text(branches, encoding="utf-8")
        gh = directory / "gh"
        gh.write_text(
            "#!/usr/bin/env bash\n"
            f'printf "%s\\n" "$@" > "{directory}/arguments"\n'
            f'cat "{directory}/branches"\n'
            f"exit {gh_status}\n",
            encoding="utf-8",
        )
        gh.chmod(0o755)
        output = directory / "output"
        output.touch()
        repository = directory / "repository"
        git = ["git", "-C", str(repository), "-c", "user.name=t", "-c", "user.email=t@localhost"]
        git += ["-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false"]
        subprocess.run(["git", "init", "--quiet", str(repository)], check=True)
        subprocess.run([*git, "commit", "--quiet", "--allow-empty", "-m", "c"], check=True)
        if tag:
            subprocess.run([*git, "tag", tag], check=True)
        environment = dict(
            os.environ,
            PATH=f"{directory}{os.pathsep}{os.environ['PATH']}",
            GITHUB_REPOSITORY="owner/demo",
            GITHUB_SHA="0123abcd",
            GITHUB_OUTPUT=str(output),
        )
        result = subprocess.run(
            ["bash", "-e", "-c", script],
            env=environment,
            cwd=repository,
            capture_output=True,
            text=True,
        )
        arguments_file = directory / "arguments"
        arguments = arguments_file.read_text().split("\n")[:-1] if arguments_file.exists() else []
        return result.returncode, output.read_text(), arguments


def check_find_step(reads_tag):
    """Checks what the finding step's script does with each answer `gh` can give, and with a tag
    on the commit, which releases only in a project whose release job reads one (`reads_tag`)."""
    for branches, tag, merged in [
        ("release-plz-2026-11-23T10-00-00Z\n", None, True),
        ("feat/readme\nrelease-plz-2026-11-23T10-00-00Z\n", None, True),
        ("feat/readme\nrenovate/toml-1.x\n", None, False),
        ("my-release-plz-x\nrelease-notes\n", None, False),
        ("", None, False),
        ("", "v0.2.0", reads_tag),
        ("feat/readme\n", "nightly", False),
    ]:
        code, output, arguments = run_find_step(branches, tag=tag)
        found = "releases=true" in output.split("\n")
        if code != 0 or found != merged:
            problems.append(
                f"the finding step exits {code} and finds {'a' if found else 'no'} release PR "
                f"for the merged branches {branches.split()} and the tag {tag}, but should exit 0 "
                "and find "
                f"{'one' if merged else 'none'}"
            )
        expected = ["api", "repos/owner/demo/commits/0123abcd/pulls", "--jq", ".[].head.ref"]
        if arguments != expected:
            problems.append(f"the finding step asks gh {arguments}, not {expected}")
    code, output, _ = run_find_step("release-plz-x\n", gh_status=1)
    if code == 0:
        problems.append("the finding step passes when gh fails, so an error would skip the check")


check_pull_request()
check_release()
check_release_plz()
for problem in dict.fromkeys(problems):
    print(f"error: {project}: {problem}", file=sys.stderr)
sys.exit(1 if problems else 0)
