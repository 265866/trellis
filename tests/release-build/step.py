"""Prints a shell script that runs one step of a workflow job outside GitHub Actions.

Usage: step.py <workflow> <job> <step name>

The script exports the step's `env` entries whose values hold no `${{ }}` expression, then runs
the step's `run` script. The caller sets the variables whose values are expressions, such as
TARGET. It exits with 2 when the job has no step of that name.
"""

import shlex
import sys

import yaml

workflow, job, name = sys.argv[1:]
with open(workflow, encoding="utf-8") as file:
    steps = yaml.safe_load(file)["jobs"][job]["steps"]
step = next((step for step in steps if step.get("name") == name), None)
if step is None:
    sys.exit(2)
for key, value in step.get("env", {}).items():
    text = str(value).lower() if isinstance(value, bool) else str(value)
    if "${{" not in text:
        print(f"export {key}={shlex.quote(text)}")
print(step["run"])
