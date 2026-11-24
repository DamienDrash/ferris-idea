#!/bin/sh
# Stand-in for the project's `prettier` in the MCP protocol test of SPEC-0123 v5: it reads standard
# input and changes just enough to show that Ferris IDEA ran it with the expected command line.
case "$1" in
  --stdin-filepath) sed 's/^let n = 1$/let n = 1;/' ;;
  *) echo "unexpected arguments: $*" >&2; exit 2 ;;
esac
