#!/usr/bin/env bash
# Reproduce the fin-primitives vs TA-Lib comparison.
# Needs: Rust (cargo), Python 3.9+ with `pip install TA-Lib numpy` (wheels bundle the C library).
set -euo pipefail
cd "$(dirname "$0")"
PY="${PYTHON:-python}"
"$PY" gen_data.py --bars 100000 --seed 20260930
cargo run --release --quiet -- data 5
"$PY" compare.py | tee data/report.md
