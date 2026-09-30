#!/bin/bash
set -e

TARGET="${1:-../komun-server}"

if [ ! -d "$TARGET" ]; then
    echo "Target directory $TARGET does not exist"
    exit 1
fi

echo "Syncing server code to $TARGET..."

rsync -av --delete --exclude='target/' crates/server/ "$TARGET/crates/server/"
rsync -av --delete --exclude='target/' crates/core/ "$TARGET/crates/core/"
rsync -av --delete migrations/ "$TARGET/migrations/"
cp config.example.toml "$TARGET/config.example.toml"

echo "Done. Remember to commit and push $TARGET separately."
