"""Answers `copier update`'s questions in a terminal, as an owner would, changing only one.

Usage: interactive-update.py <answer>

Runs `copier update --vcs-ref=:current:` in the current directory in a pseudo-terminal, accepts
each question's stored answer, and types <answer> at the `release` question. It prints and exits
with what Copier did with it: 0 for `refused`, when Copier shows the answer's check failing on the
`app_id` the answers file holds, and 1 for `accepted`, when the next question or the end comes
instead. It stops the update either way, so it changes no file.
"""

import sys
import time

import pexpect

QUESTION = "\U0001f3a4"
answer = sys.argv[1]
child = pexpect.spawn(
    "uv",
    ["tool", "run", "copier@9.19.1", "update", "--vcs-ref=:current:"],
    encoding="utf-8",
    timeout=120,
    dimensions=(50, 1000),
)
while True:
    if child.expect([QUESTION, pexpect.EOF]) == 1:
        sys.exit("error: interactive update: the update ended before the release question")
    child.expect("\r\n")
    question = child.before
    time.sleep(0.5)
    if "releases attach" not in question:
        child.send("\r")
        child.expect(QUESTION)
        child.expect("\r\n")
        continue
    # Ctrl+E and Ctrl+U clear the stored answer the prompt shows.
    child.send("\x05\x15" + answer + "\r")
    outcome = child.expect([r"in \.copier-answers\.yml", QUESTION, pexpect.EOF], timeout=30)
    child.sendcontrol("c")
    child.close(force=True)
    print("refused" if outcome == 0 else "accepted")
    sys.exit(0 if outcome == 0 else 1)
