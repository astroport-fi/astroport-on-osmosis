#!/usr/bin/env bash

set -e
set -o pipefail

# osmosis v22.0.5: https://github.com/osmosis-labs/wasmd/blob/aba521a80563ceb88d27d14e5d7527735f4aae5d/x/wasm/types/validation.go#L22
maximum_size=800

for artifact in artifacts/*.wasm; do
  artifactsize=$(du -k "$artifact" | cut -f 1)
  if [ "$artifactsize" -gt $maximum_size ]; then
    echo "Artifact file size exceeded: $artifact"
    echo "Artifact size: $artifactsize"
    echo "Max size: $maximum_size"
    exit 1
  fi
done
