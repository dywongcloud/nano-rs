#!/bin/bash
# Wrapper script for zig as a linker
# Filters out incompatible arguments

args=()
for arg in "$@"; do
    # Skip arguments that zig doesn't understand
    case "$arg" in
        -m64) ;;  # Skip -m64
        -arch) ;;  # Skip -arch and its value
        *) args+=("$arg") ;;
    esac
done

exec /opt/homebrew/opt/zig@0.15/bin/zig cc "${args[@]}"
