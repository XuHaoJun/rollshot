#!/usr/bin/env bash
set -euo pipefail

paths=(
  crates/rollshot-app/src
  crates/rollshot-core/src
  crates/rollshot-capture/src
  crates/rollshot-iced-overlay/src
)

status=0

if rg -U --pcre2 -n \
  'tracing::(?:trace|debug|info|warn|error|event)!\(\s*+(?!target:)' \
  "${paths[@]}"; then
  echo "tracing macros must begin with an explicit target:" >&2
  status=1
fi

if rg -n --pcre2 \
  '(^|[^:[:alnum:]_])(trace|debug|info|warn|error|event)!\(' \
  "${paths[@]}"; then
  echo "use fully-qualified tracing macros so the target check can inspect them" >&2
  status=1
fi

exit "$status"
