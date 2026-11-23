"""Checks that a generated project can't publish a release while the README is unwritten.

Usage: gate.py <project>

`just policy-release` fails on what `just policy` only notes, such as a README prompt left to
write. This checks in <project> that both release routes run it:

- The release pull request: the quick job of `.github/workflows/ci.yml` runs it on every pull
  request whose head branch starts with `release-plz-`, the branch release-plz opens and the one
  a person makes by hand, and on no other.
- The release itself: the release job of `.github/workflows/release.yml` runs it before the
  release-plz step that tags and publishes, when the step that finds whether this push releases
  finds a pull request from a `release-plz-` branch, which is when release-plz releases, or, in a
  project whose release job reads a `v*` tag on the commit to build and publish, when the commit
  has one. The step asks GitHub which pull requests the commit belongs to, and also which branch
  the pull request whose number ends the commit's subject, ` (#N)`, came from, counting it only
  when that pull request was merged as this commit, since the first answer can lag just after
  the merge and a subject can be edited, and not when GitHub finds no such pull request, as for
  an issue's number. A library also checks a push without a number that counts, apart from the
  push that creates the branch, which `CREATED`, bound to `github.event.created`, tells it.
  In a project whose release job reads the tag, a second check runs after release-plz whenever
  release-plz released or a tag is left to publish and the first check didn't run; the test
  evaluates both checks' conditions against those outputs. Each check installs just before it,
  neither may continue on an error, every other job needs the release job, and none runs when
  it fails, so nothing publishes first. The finding step's own script, run as GitHub runs it in a
  git repository with a stand-in `gh`, finds a merged `release-plz-` branch from either answer
  and only that, and the tag where the project reads one, asks for this repository's commit and
  the subject's pull request, and fails when either call of `gh` does, so an error never skips
  the check.
- release-plz.toml releases only those pushes: its `pr_branch_prefix` is the default,
  `release-plz-`, and it sets `release_always = false`.

It prints each break and exits with 1 when there is one.
"""

import os
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib

import yaml

STRICT = "just policy-release"
PR_CONDITION = "github.event_name == 'pull_request' && startsWith(github.head_ref, 'release-plz-')"
FOUND = "steps.releases.outputs.releases == 'true'"
STRICT_STEP = "Check the docs as a release does"
AFTER_STEP = "Check the docs as a release does, after release-plz released"
AFTER = (
    "steps.releases.outputs.releases != 'true' && (steps.tag.outputs.tag != '' || "
    "(steps.release-plz.outputs.releases != '' && steps.release-plz.outputs.releases != '[]'))"
)
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


def is_just_install(step):
    """Whether `step` installs just."""
    return str(step.get("uses", "")).startswith("taiki-e/install-action@") and "just@" in str(
        step.get("with", {}).get("tool", "")
    )


def check_strict_step(steps, name, condition, after):
    """Checks that `steps` run `just policy-release` in exactly one step named `name`, under
    `if: condition`, not continuing on an error, after the step at index `after`, with just
    installed before it whenever it runs. Returns its index, or None."""
    strict = index_of(steps, lambda step: step.get("name") == name)
    if strict is None or steps[strict].get("run") != STRICT:
        problems.append(f"release.yml's release job has no single step `{name}` that runs `{STRICT}`")
        return None
    if steps[strict].get("continue-on-error"):
        problems.append(f"release.yml's `{name}` step continues on an error")
    if steps[strict].get("if") != condition:
        problems.append(
            f"release.yml runs `{name}` under `if: {steps[strict].get('if')}`, not `if: {condition}`"
        )
    if after is None or strict < after:
        problems.append(f"release.yml runs `{name}` before the step it must follow")
    installed = any(
        is_just_install(step) and step.get("if") in (None, condition) for step in steps[:strict]
    )
    if not installed:
        problems.append(f"release.yml doesn't install just before `{name}` whenever it runs it")
    return strict


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
    if publish is None:
        problems.append("release.yml's release job has no single release-plz `release` step")
        return
    if find is None or steps[find].get("name") != FIND_STEP:
        problems.append(f"release.yml's release job has no step `{FIND_STEP}` with id releases")
    strict = check_strict_step(steps, STRICT_STEP, FOUND, find)
    if strict is not None and strict > publish:
        problems.append(
            f"release.yml runs `{STRICT_STEP}` after the release-plz step that tags and publishes"
        )
    tag = index_of(steps, lambda step: step.get("id") == "tag")
    reads_tag = tag is not None
    if reads_tag:
        after = check_strict_step(steps, AFTER_STEP, AFTER, tag)
        if strict is not None and after is not None:
            check_conditions(steps[strict]["if"], steps[after]["if"])
    for name, job in jobs.items():
        seen, todo = set(), needs(job)
        while todo:
            other = todo.pop()
            if other not in seen:
                seen.add(other)
                todo.extend(needs(jobs.get(other, {})))
        if name != "release" and "release" not in seen:
            problems.append(f"release.yml's {name} job can run before the release job's check")
        condition = str(job.get("if", ""))
        if name != "release" and any(
            status in condition for status in ("always()", "failure()", "cancelled()")
        ):
            problems.append(f"release.yml's {name} job runs even when the release job's check fails")
    if find is not None:
        created = steps[find].get("env", {}).get("CREATED")
        if reads_tag and created is not None:
            problems.append("release.yml's finding step reads `CREATED`, which only a library's needs")
        if not reads_tag and created != CREATED:
            problems.append(
                f"a library's finding step sets `CREATED` to {created!r}, not `{CREATED}`, so it "
                "can't tell the push that creates `main`"
            )
    if find is not None and steps[find].get("name") == FIND_STEP:
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


def evaluate(condition, outputs):
    """The value of `condition`, a GitHub Actions `if` of `steps.<id>.outputs.<name>` compared
    with `==` and `!=` to string literals and joined with `&&`, `||` and parentheses, when the
    outputs are `outputs`, keyed by `(id, name)`, and an output it lacks is empty."""
    python = re.sub(
        r"steps\.([\w-]+)\.outputs\.([\w-]+)",
        lambda match: repr(outputs.get((match[1], match[2]), "")),
        condition,
    )
    python = python.replace("&&", " and ").replace("||", " or ")
    return eval(python, {"__builtins__": {}})  # noqa: S307, the workflow's own condition


def check_conditions(before, after):
    """Checks that the strict checks' conditions, `before` release-plz and `after` it, run the
    check on every push that releases: one of them runs when release-plz released or a tag is
    left to publish, and the one after release-plz runs only when the one before didn't."""
    released = '[{"package_name": "demo", "tag": "v0.2.0"}]'
    for name, outputs, runs_before, runs_after in [
        (
            "release-plz released a push the finding step missed",
            {("release-plz", "releases"): released, ("tag", "tag"): "v0.2.0"},
            False,
            True,
        ),
        (
            "a re-run found a tag left to publish",
            {("release-plz", "releases"): "[]", ("tag", "tag"): "v0.2.0"},
            False,
            True,
        ),
        (
            "the finding step found the release",
            {
                ("releases", "releases"): "true",
                ("release-plz", "releases"): released,
                ("tag", "tag"): "v0.2.0",
            },
            True,
            False,
        ),
        ("nothing released", {("release-plz", "releases"): "[]"}, False, False),
    ]:
        if (evaluate(before, outputs), evaluate(after, outputs)) != (runs_before, runs_after):
            should = {True: "run", False: "be skipped"}
            problems.append(
                f"when {name}, the check before release-plz should {should[runs_before]} and the "
                f"one after it should {should[runs_after]}, but their conditions say otherwise"
            )


SHA = "0123abcd"


NOT_FOUND = "gh: Not Found (HTTP 404)"
SERVER_ERROR = "gh: Server Error (HTTP 500)"
CREATED = "${{ github.event.created }}"


def merged(branch, sha=SHA):
    """What `gh` says of a pull request from `branch` merged as the commit `sha`."""
    return f"{branch}\ttrue\t{sha}"


def run_find_step(branches, subject, pull=None, gh_status=0, tag=None, created=False):
    """Runs the finding step's script as GitHub runs it, in a git repository whose commit has the
    subject `subject` and the tag `tag`, if any, on a push that created the branch if `created`,
    with a `gh` that answers the pull requests of the commit with `branches`, exiting with
    `gh_status`, and a pull request with `pull`, its branch, whether it was merged and its merge
    commit separated by tabs, failing when that is None. Returns its exit code, what it wrote to
    GITHUB_OUTPUT and the command line of each call of `gh`."""
    script = subprocess.run(
        [sys.executable, "-I", str(STEP_SCRIPT), str(workflows / "release.yml"), "release", FIND_STEP],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    with tempfile.TemporaryDirectory() as directory:
        directory = pathlib.Path(directory)
        (directory / "branches").write_text(branches, encoding="utf-8")
        if pull in (None, NOT_FOUND):
            failure = SERVER_ERROR if pull is None else NOT_FOUND
            (directory / "pull-error").write_text(f"{failure}\n", encoding="utf-8")
        else:
            (directory / "pull").write_text(f"{pull}\n", encoding="utf-8")
        gh = directory / "gh"
        gh.write_text(
            "#!/usr/bin/env bash\n"
            f'printf "%s\\n" "$*" >> "{directory}/calls"\n'
            'case "$2" in\n'
            f'  */commits/*/pulls) cat "{directory}/branches"; exit {gh_status} ;;\n'
            f'  */pulls/*) cat "{directory}/pull" 2> /dev/null'
            f' || {{ cat "{directory}/pull-error" >&2; exit 1; }} ;;\n'
            "  *) exit 1 ;;\n"
            "esac\n",
            encoding="utf-8",
        )
        gh.chmod(0o755)
        output = directory / "output"
        output.touch()
        repository = directory / "repository"
        git = ["git", "-C", str(repository), "-c", "user.name=t", "-c", "user.email=t@localhost"]
        git += ["-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false"]
        subprocess.run(["git", "init", "--quiet", str(repository)], check=True)
        subprocess.run([*git, "commit", "--quiet", "--allow-empty", "-m", subject], check=True)
        if tag:
            subprocess.run([*git, "tag", tag], check=True)
        environment = dict(
            os.environ,
            PATH=f"{directory}{os.pathsep}{os.environ['PATH']}",
            GITHUB_REPOSITORY="owner/demo",
            GITHUB_SHA=SHA,
            GITHUB_OUTPUT=str(output),
            CREATED="true" if created else "false",
        )
        result = subprocess.run(
            ["bash", "-e", "-c", script],
            env=environment,
            cwd=repository,
            capture_output=True,
            text=True,
        )
        calls_file = directory / "calls"
        calls = calls_file.read_text().split("\n")[:-1] if calls_file.exists() else []
        return result.returncode, output.read_text(), calls


def check_find_step(reads_tag):
    """Checks what the finding step's script does with each answer `gh` can give, with the
    commit's subject, and with a tag on the commit, which releases only in a project whose release
    job reads one (`reads_tag`). A subject's number counts only for a pull request merged as this
    commit. A subject without a number that counts is a push straight to `main` or an edited
    one: a library checks it, unless the push created `main`, since it has no later check, and a
    binary or service leaves it to the check after release-plz."""
    release_pr = "release-plz-2026-11-23T10-00-00Z"
    commits = "api repos/owner/demo/commits/0123abcd/pulls --jq .[].head.ref"
    library = not reads_tag
    for branches, subject, pull, tag, created, releases in [
        (f"{release_pr}\n", "chore: release v0.2.0 (#7)", merged(release_pr), None, False, True),
        (f"feat/a\n{release_pr}\n", "feat: write it (#7)", merged("feat/a"), None, False, True),
        ("feat/a\nrenovate/toml-1.x\n", "feat: write it (#7)", merged("feat/a"), None, False, False),
        ("my-release-plz-x\nrelease-notes\n", "feat: a (#7)", merged("release-notes"), None, False, False),
        ("", "chore: release v0.2.0 (#7)", merged(release_pr), None, False, True),
        ("", "feat: write it (#7)", merged("feat/a"), None, False, False),
        ("", "feat: write it (#7)", merged("feat/a"), "v0.2.0", False, reads_tag),
        ("feat/a\n", "feat: write it (#7)", merged("feat/a"), "nightly", False, False),
        ("", "fix: x (#3) (#7)", merged(release_pr), None, False, True),
        ("", "a push straight to main", None, None, False, library),
        ("", "fix: what #7 found (#7) and more", None, None, False, library),
        ("", "chore: generate from trellis", None, None, True, False),
        ("", "chore: release v0.2.0 (#7)", merged(release_pr, "fedcba98"), None, False, library),
        ("", "chore: release v0.2.0 (#7)", merged("feat/a", "fedcba98"), None, False, library),
        ("", "chore: release v0.2.0 (#7)", f"{release_pr}\tfalse\t{SHA}", None, False, library),
        ("", "chore: release v0.2.0 (#7)", f"feat/a\tfalse\t{SHA}", None, False, library),
        ("", "fix: what an issue reported (#7)", NOT_FOUND, None, False, library),
    ]:
        code, output, calls = run_find_step(branches, subject, pull, tag=tag, created=created)
        found = "releases=true" in output.split("\n")
        if code != 0 or found != releases:
            problems.append(
                f"the finding step exits {code} and {'finds' if found else 'finds no'} release "
                f"for the merged branches {branches.split()}, the subject {subject!r}, the pull "
                f"request {pull!r}, the tag {tag} and a push that "
                f"{'created' if created else 'updated'} the branch, but should exit 0 and "
                f"{'find one' if releases else 'find none'}"
            )
        expected = [commits]
        if pull is not None:
            expected.append(
                "api repos/owner/demo/pulls/7 --jq [.head.ref, .merged, .merge_commit_sha] | @tsv"
            )
        if calls != expected:
            problems.append(f"the finding step asks gh {calls}, not {expected}")
    code, _, _ = run_find_step(f"{release_pr}\n", "chore: release (#7)", merged(release_pr), 1)
    if code == 0:
        problems.append("the finding step passes when gh fails, so an error would skip the check")
    code, _, _ = run_find_step("", "chore: release v0.2.0 (#7)", pull=None)
    if code == 0:
        problems.append(
            "the finding step passes when gh fails to read the pull request the subject names "
            "for a reason other than its absence, such as HTTP 500, so an error would skip the "
            "check"
        )


check_pull_request()
check_release()
check_release_plz()
for problem in dict.fromkeys(problems):
    print(f"error: {project}: {problem}", file=sys.stderr)
sys.exit(1 if problems else 0)
