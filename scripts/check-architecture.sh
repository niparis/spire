#!/usr/bin/env bash
set -euo pipefail

metadata="$(cargo metadata --no-deps --format-version=1)"

for crate in spire-domain spire-application; do
  forbidden='["axum", "reqwest", "sqlx", "tokio"]'
  if [[ "$crate" == "spire-application" ]]; then
    forbidden='["axum", "reqwest", "sqlx", "tokio", "lineark-sdk"]'
  fi

  printf '%s' "$metadata" | jq -e --arg crate "$crate" --argjson forbidden "$forbidden" '
    [.packages[] | select(.name == $crate) | .dependencies[].name] as $dependencies |
    [$dependencies[] | select(. as $dependency | $forbidden | index($dependency))] | length == 0
  ' >/dev/null
done

printf 'architecture dependency boundaries pass\n'
