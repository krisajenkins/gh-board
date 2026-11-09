#!/usr/bin/env bash
# Validate every GraphQL document against GitHub's live schema, without side
# effects: each request uses made-up ids, so a valid document fails with
# NOT_FOUND (or a missing org/repo), and an invalid one with a validation error.
set -euo pipefail
cd "$(dirname "$0")/.."

bad=0
while IFS=$'\t' read -r name json; do
  out=$(gh api graphql --input - <<<"$json" 2>/dev/null || true)
  if ! jq -e 'has("data") or has("errors")' <<<"$out" >/dev/null 2>&1; then
    printf 'FAIL  %-16s no GraphQL response (is gh logged in?)\n' "$name"
    bad=1
    continue
  fi
  types=$(jq -r '[.errors[]? | .type // .extensions.code // "UNKNOWN"] | unique | join(",")' <<<"$out")
  case "$types" in
    NOT_FOUND|"") printf 'ok    %-16s %s\n' "$name" "$types" ;;
    *)
      printf 'FAIL  %-16s %s\n' "$name" "$types"
      jq -r '.errors[]?.message' <<<"$out" | sed 's/^/        /'
      bad=1
      ;;
  esac
done < <(cargo run --quiet --example requests)
exit "$bad"
