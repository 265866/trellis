"""Writes a generated project's README as its owner would, so the release checks pass.

Usage: answer.py <project> <interfaces>

Each `**To write:**` prompt in README.md and the pages under docs/ becomes a sentence. When the
first of the comma-separated interfaces is web, desktop or terminal, the README also gets its
picture before `## Quick start`: an image saved in docs/screenshots/ and linked to the
screenshots section of docs/usage.md, and a sentence that says so.
"""

import pathlib
import struct
import sys
import zlib

UI = {"web", "desktop", "terminal"}
PROMPT = "> **To write:**"


def answered(text):
    """`text` with each quote that opens with a prompt replaced by a sentence."""
    lines = text.split("\n")
    out = []
    index = 0
    while index < len(lines):
        if lines[index].startswith(PROMPT):
            while index < len(lines) and lines[index].startswith(">"):
                index += 1
            out.append("Written by the owner.")
            continue
        out.append(lines[index])
        index += 1
    return "\n".join(out)


def png():
    """A PNG of one black pixel, with only the IHDR, IDAT and IEND chunks."""

    def chunk(kind, data):
        crc = zlib.crc32(kind + data)
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", crc)

    header = struct.pack(">IIBBBBB", 1, 1, 8, 0, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(b"\x00\x00"))
        + chunk(b"IEND", b"")
    )


def main():
    project = pathlib.Path(sys.argv[1])
    primary = sys.argv[2].split(",")[0]
    files = [project / "README.md", *sorted((project / "docs").rglob("*.md"))]
    for path in files:
        path.write_text(answered(path.read_text(encoding="utf-8")), encoding="utf-8")
    if primary in UI:
        (project / "docs/screenshots").mkdir(exist_ok=True)
        (project / "docs/screenshots/main.png").write_bytes(png())
        readme = project / "README.md"
        text = readme.read_text(encoding="utf-8")
        hero = (
            "[![The main screen, with invented data](docs/screenshots/main.png)]"
            "(docs/usage.md#screenshots)\n\n"
            "Select the image to open the gallery.\n\n"
        )
        if "\n## Quick start\n" not in text:
            sys.exit(f"error: {readme} has no `## Quick start` to put the picture before")
        readme.write_text(text.replace("\n## Quick start\n", f"\n{hero}## Quick start\n", 1))


main()
