#!/bin/bash
set -e
echo "=== cargo audit ==="
cargo audit || true
echo "=== npm audit (web) ==="
cd web && npm audit --production || true
