#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[audit] Phase-4 legacy cleanup checks"

if [[ -d "frontend" || -d "backend" ]]; then
  echo "[error] legacy directories still exist (frontend/ or backend/)"
  exit 1
fi

legacy_tracked="$(git ls-files | grep '\.(ts|tsx|js|py)$' || true)"
if [[ -n "$legacy_tracked" ]]; then
  echo "[error] legacy tracked files remain:"
  echo "$legacy_tracked"
  exit 1
fi

echo "[audit] Runtime/deploy guardrail checks"
grep -q '^WS_AUTH_MODE=oidc_broker$' .env.example
grep -q '^OIDC_ALLOW_INSECURE_DEV_TOKENS=0$' .env.example
grep -q '^SINGLE_INSTANCE_ONLY=1$' .env.example
grep -q -- '--max-instances=1' scripts/deploy_cloudrun.sh
grep -q 'WS_AUTH_MODE=oidc_broker' scripts/deploy_cloudrun.sh
grep -q -- '--max-instances=1' .github/workflows/deploy.yml
grep -q 'WS_AUTH_MODE=oidc_broker' .github/workflows/deploy.yml
grep -q 'max_instance_count = 1' infra/cloud_run.tf
grep -q 'SINGLE_INSTANCE_ONLY' infra/cloud_run.tf
grep -q '^\*-firebase-adminsdk-\*\.json$' .dockerignore

echo "[audit] Rust quality gates"
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

echo "[audit] Dependency sanity checks"
if cargo tree --workspace -d | grep -qE 'tokio-tungstenite v0\.24|tungstenite v0\.24'; then
  echo "[error] legacy tungstenite 0.24 branch still present"
  exit 1
fi

echo "[ok] Release audit passed"
