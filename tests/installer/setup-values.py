"""Prints what an Inno Setup script sets, from its preprocessed text, for comparing two scripts.

Usage: setup-values.py <preprocessed script>

`ISCC -e` (Inno Setup 7.1 and later) preprocesses a script, `#define`s and `#include`s resolved,
without compiling it, so nothing here checks that `[Code]` parses or that the script builds. This
prints each `[Setup]` directive the script sets, with a note on any set twice, which the compiler
refuses; then each entry of the other sections, one per line, apart from `[Code]`, of which it
prints the header of each function and procedure and each line that reads a command-line
parameter. Each line starts with its section, so once sorted, as setup-values.sh sorts them, a
`diff` lines two scripts up entry by entry.
"""

import re
import sys

text = open(sys.argv[1], encoding="utf-8-sig").read()
section = None
setup = {}
lines = []
for raw in text.splitlines():
    line = raw.strip()
    if not line or line.startswith(";") or (section != "code" and line.startswith("//")):
        continue
    header = re.fullmatch(r"\[([A-Za-z]+)\]", line)
    if header:
        section = header.group(1).lower()
        continue
    if section == "setup":
        key, _, value = line.partition("=")
        key = key.strip()
        if key.lower() in setup:
            lines.append(f"setup  {key}: set again, which the compiler refuses: {value.strip()}")
        setup[key.lower()] = (key, value.strip())
    elif section == "code":
        if re.match(r"(function|procedure)\s", line, re.IGNORECASE) or "ParamStr" in line:
            lines.append(f"code   {line}")
    elif section is not None:
        lines.append(f"{section:6} {line}")
for key, value in sorted(setup.values(), key=lambda item: item[0].lower()):
    print(f"setup  {key}={value}")
for line in lines:
    print(line)
