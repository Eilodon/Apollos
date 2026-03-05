#!/usr/bin/env bash
set -euo pipefail

# Rotate both:
# 1) Firebase service-account JSON key
# 2) Gemini API key used in backend/.env (GEMINI_API_KEY)
#
# Requirements:
# - gcloud CLI installed
# - Network access to Google APIs
# - IAM permissions:
#   iam.serviceAccountKeys.create/delete
#   apikeys.keys.create/delete/getKeyString

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PROJECT_ID="${PROJECT_ID:-apollos-c7028}"
BACKEND_ENV_FILE="${BACKEND_ENV_FILE:-$REPO_ROOT/backend/.env}"
SERVICE_ACCOUNT_KEY_FILE="${SERVICE_ACCOUNT_KEY_FILE:-}"
SKIP_DELETE_OLD_KEYS="${SKIP_DELETE_OLD_KEYS:-0}"

if [[ -z "$SERVICE_ACCOUNT_KEY_FILE" && -f "$BACKEND_ENV_FILE" ]]; then
  SERVICE_ACCOUNT_KEY_FILE="$(
    awk -F= '/^GOOGLE_APPLICATION_CREDENTIALS=/{v=$2; gsub(/^"|"$/, "", v); print v}' "$BACKEND_ENV_FILE" | tail -n1
  )"
fi
SERVICE_ACCOUNT_KEY_FILE="${SERVICE_ACCOUNT_KEY_FILE:-$HOME/.config/apollos/firebase-adminsdk.json}"

if [[ ! -f "$SERVICE_ACCOUNT_KEY_FILE" ]]; then
  echo "Missing service-account key file: $SERVICE_ACCOUNT_KEY_FILE"
  exit 1
fi
if [[ ! -f "$BACKEND_ENV_FILE" ]]; then
  echo "Missing backend env file: $BACKEND_ENV_FILE"
  exit 1
fi

SERVICE_ACCOUNT_EMAIL="$(
  SERVICE_ACCOUNT_KEY_FILE="$SERVICE_ACCOUNT_KEY_FILE" python3 - <<'PY'
import json
import os
from pathlib import Path
obj = json.loads(Path(os.environ["SERVICE_ACCOUNT_KEY_FILE"]).read_text())
print(obj.get("client_email", ""))
PY
)"
OLD_FIREBASE_KEY_ID="$(
  SERVICE_ACCOUNT_KEY_FILE="$SERVICE_ACCOUNT_KEY_FILE" python3 - <<'PY'
import json
import os
from pathlib import Path
obj = json.loads(Path(os.environ["SERVICE_ACCOUNT_KEY_FILE"]).read_text())
print(obj.get("private_key_id", ""))
PY
)"

if [[ -z "$SERVICE_ACCOUNT_EMAIL" ]]; then
  echo "Cannot parse client_email from $SERVICE_ACCOUNT_KEY_FILE"
  exit 1
fi

echo "Setting GCP project..."
gcloud config set project "$PROJECT_ID" >/dev/null

echo "Creating new Firebase service-account key..."
NEW_FIREBASE_KEY_FILE="$(mktemp /tmp/apollos-firebase-key.XXXXXX.json)"
gcloud iam service-accounts keys create "$NEW_FIREBASE_KEY_FILE" \
  --iam-account="$SERVICE_ACCOUNT_EMAIL" >/dev/null

chmod 600 "$NEW_FIREBASE_KEY_FILE"
mv "$NEW_FIREBASE_KEY_FILE" "$SERVICE_ACCOUNT_KEY_FILE"
chmod 600 "$SERVICE_ACCOUNT_KEY_FILE"
echo "Replaced service-account key file at $SERVICE_ACCOUNT_KEY_FILE"

if [[ "$SKIP_DELETE_OLD_KEYS" != "1" && -n "$OLD_FIREBASE_KEY_ID" ]]; then
  echo "Deleting old Firebase key id: $OLD_FIREBASE_KEY_ID"
  gcloud iam service-accounts keys delete "$OLD_FIREBASE_KEY_ID" \
    --iam-account="$SERVICE_ACCOUNT_EMAIL" --quiet >/dev/null || true
fi

OLD_GEMINI_API_KEY="$(awk -F= '/^GEMINI_API_KEY=/{v=$2; gsub(/^"|"$/, "", v); print v}' "$BACKEND_ENV_FILE" | tail -n1)"
if [[ -z "$OLD_GEMINI_API_KEY" ]]; then
  echo "Cannot find GEMINI_API_KEY in $BACKEND_ENV_FILE"
  exit 1
fi

echo "Creating new Gemini API key..."
NEW_KEY_OUTPUT="$(gcloud services api-keys create \
  --display-name="apollos-gemini-$(date +%Y%m%d-%H%M%S)" \
  --project="$PROJECT_ID" 2>&1 || true)"
NEW_GEMINI_API_KEY="$(echo "$NEW_KEY_OUTPUT" | grep -o '"keyString":"[^"]*"' | head -n1 | cut -d'"' -f4)"

if [[ -z "$NEW_GEMINI_API_KEY" ]]; then
  echo "Failed to create new Gemini API key"
  exit 1
fi

BACKEND_ENV_FILE="$BACKEND_ENV_FILE" NEW_GEMINI_API_KEY="$NEW_GEMINI_API_KEY" python3 - <<'PY'
from pathlib import Path
import os
import re

path = Path(os.environ["BACKEND_ENV_FILE"])
text = path.read_text()
new_key = os.environ["NEW_GEMINI_API_KEY"]
updated = re.sub(r'^GEMINI_API_KEY=.*$', f'GEMINI_API_KEY="{new_key}"', text, flags=re.M)
if updated == text:
    raise SystemExit("GEMINI_API_KEY line not found for update")
path.write_text(updated)
PY
chmod 600 "$BACKEND_ENV_FILE"
echo "Updated GEMINI_API_KEY in $BACKEND_ENV_FILE"

if [[ "$SKIP_DELETE_OLD_KEYS" != "1" ]]; then
  echo "Looking up old Gemini API key resource..."
  OLD_KEY_RESOURCE="$(gcloud services api-keys lookup "$OLD_GEMINI_API_KEY" \
    --project="$PROJECT_ID" --format='value(name)' 2>/dev/null || true)"
  if [[ -n "$OLD_KEY_RESOURCE" ]]; then
    echo "Deleting old Gemini API key resource..."
    gcloud services api-keys delete "$OLD_KEY_RESOURCE" \
      --project="$PROJECT_ID" --quiet >/dev/null || true
  fi
fi

echo "Rotation completed."
echo "- Firebase key: rotated"
echo "- GEMINI_API_KEY: rotated and backend/.env updated"
echo "Run backend restart/redeploy to apply the new key."
