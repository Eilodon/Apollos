#!/usr/bin/env bash

# Example deployment script for Google Cloud Run
set -e

PROJECT_ID="apollos-c7028"
REGION="us-central1"
SERVICE_NAME="apollos-backend"
FRONTEND_ORIGIN="${FRONTEND_ORIGIN:-https://example.com}"

: "${GEMINI_API_KEY:?Set GEMINI_API_KEY env var before deploying}"
: "${OIDC_ISSUER:?Set OIDC_ISSUER env var before deploying}"
: "${OIDC_AUDIENCE:?Set OIDC_AUDIENCE env var before deploying}"

echo "Deploying Apollos Backend to Cloud Run..."

# Build and deploy the FastAPI backend container
# Critical Flags for WebSocket & Gemini Live:
# --use-http2: Enables HTTP/2 for better bidirectional stream support
# --session-affinity: Required if you ever run >1 instance so WebSocket clients stick to the same server
# --timeout=3600: Prevents Cloud Run from aggressively killing long-running Gemini Live sessions

gcloud run deploy ${SERVICE_NAME} \
  --source ./backend \
  --region ${REGION} \
  --project ${PROJECT_ID} \
  --allow-unauthenticated \
  --session-affinity \
  --timeout=3600 \
  --cpu=2 \
  --memory=2Gi \
  --set-env-vars="APP_ENV=production,ENABLE_GEMINI_LIVE=1,GEMINI_MODEL=gemini-live-2.5-flash-native-audio,GEMINI_API_KEY=${GEMINI_API_KEY},USE_FIRESTORE=1,GOOGLE_CLOUD_PROJECT=apollos-c7028,WS_AUTH_MODE=oidc,OIDC_ISSUER=${OIDC_ISSUER},OIDC_AUDIENCE=${OIDC_AUDIENCE},ENABLE_DEV_ENDPOINTS=0,CORS_ALLOW_ORIGINS=${FRONTEND_ORIGIN}"

echo "✅ Deployment complete!"
