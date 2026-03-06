#!/usr/bin/env bash
set -euo pipefail

export APP_ENV=development
export ENABLE_GEMINI_LIVE=0
export OIDC_ALLOW_INSECURE_DEV_TOKENS=1
export TWILIO_REQUIRED=0
export SINGLE_INSTANCE_ONLY=1
export WS_ALLOW_QUERY_TOKEN=1

cargo test -p apollos-server --test native_ws_smoke --offline "$@"
