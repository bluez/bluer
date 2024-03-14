#!/bin/bash
#
# Copies BlueR sources to remote host for testing.
#

SCRIPT_PATH="$(dirname "${BASH_SOURCE[0]}")"
TARGET="$1"
TARGET_PATH="$2"

if [ -z "$TARGET" ] ; then
    echo "target host must be specified"
    exit 1
fi

if [ -z "$TARGET_PATH" ] ; then
    TARGET_PATH="dev/bluer"
fi

rsync -avz --delete --exclude target --exclude .git "$SCRIPT_PATH/.." "$TARGET:$TARGET_PATH"
