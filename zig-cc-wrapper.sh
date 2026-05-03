#!/bin/bash
# Wrapper script for zig cc to handle --target argument correctly
# zig expects: -target x86_64-linux-gnu
# cargo passes: --target=x86_64-unknown-linux-gnu

# Convert arguments: replace --target=ARCH-VENDOR-OS-ENV with -target ARCH-OS-ENV
args=()
for arg in "$@"; do
    if [[ "$arg" == --target=* ]]; then
        # Extract target and convert format
        target="${arg#--target=}"
        # Convert x86_64-unknown-linux-gnu to x86_64-linux-gnu
        target=$(echo "$target" | sed 's/-unknown-/-/')
        args+=("-target" "$target")
    else
        args+=("$arg")
    fi
done

exec /opt/homebrew/opt/zig@0.15/bin/zig cc "${args[@]}"
