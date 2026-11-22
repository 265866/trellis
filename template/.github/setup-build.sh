#!/usr/bin/env bash
# Installs what CI's builds and the release's need beyond Rust, such as system libraries. They run
# it on every runner before they build, with the runner's system in RUNNER_OS and the target in
# TARGET. trellis wrote this starter, which installs nothing; an update leaves your version alone.
set -euo pipefail
case "$RUNNER_OS" in
  Linux) ;;
  macOS) ;;
  Windows) ;;
esac
