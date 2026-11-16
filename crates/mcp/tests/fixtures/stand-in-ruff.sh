#!/bin/sh
# Stand-in for `ruff` in the MCP protocol test of SPEC-0329: it reads standard input and changes
# just enough to show that IDECraft ran it with the expected command line.
case "$1" in
  format) sed 's/x=1/x = 1/' ;;
  check) grep -v '^import os$' ;;
  *) echo "unexpected arguments: $*" >&2; exit 2 ;;
esac
