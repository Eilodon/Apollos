#!/usr/bin/env bash

set -euo pipefail

PROJECT_ID="${PROJECT_ID:-apollos-c7028}"
ENV_FILE="${ENV_FILE:-.env.local}"

echo "Rotating Gemini API key for project: ${PROJECT_ID}"

if ! command -v gcloud >/dev/null 2>&1; then
  echo "gcloud CLI is required"
  exit 1
fi

NEW_KEY="$({
  gcloud alpha services api-keys create \
    --project "$PROJECT_ID" \
    --display-name "apollos-gemini-$(date +%Y%m%d-%H%M%S)" \
    --format='value(keyString)'
} 2>/dev/null || true)"

if [ -z "${NEW_KEY}" ]; then
  echo "Failed to create API key via gcloud alpha services api-keys create"
  exit 1
fi

touch "$ENV_FILE"
if grep -q '^GEMINI_API_KEY=' "$ENV_FILE"; then
  sed -i "s#^GEMINI_API_KEY=.*#GEMINI_API_KEY=${NEW_KEY}#" "$ENV_FILE"
else
  echo "GEMINI_API_KEY=${NEW_KEY}" >> "$ENV_FILE"
fi

echo "Updated ${ENV_FILE} with new GEMINI_API_KEY"
