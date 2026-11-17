#!/usr/bin/env bash
# Records every GIF of the docs site. Needs the seeded stack and a debug build:
#   docker compose up -d && cargo build && website/tapes/record.sh [name...]
set -euo pipefail
cd "$(dirname "$0")/../.."
names=("$@")
[[ ${#names[@]} -gt 0 ]] || names=(navigation command-bar query editing sort-page-export schema redis rabbitmq read-only)
for n in "${names[@]}"; do
  echo "recording $n"
  vhs "website/tapes/$n.tape"
done
