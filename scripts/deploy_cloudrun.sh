#!/usr/bin/env bash

set -euo pipefail

PROJECT_ID="${PROJECT_ID:-apollos-c7028}"
REGION="${REGION:-us-central1}"
SERVICE_NAME="${SERVICE_NAME:-apollos-server}"
FRONTEND_ORIGIN="${FRONTEND_ORIGIN:-https://example.com}"

: "${GEMINI_API_KEY:?Set GEMINI_API_KEY before deploying}"
: "${OIDC_ISSUER:?Set OIDC_ISSUER before deploying}"
: "${OIDC_AUDIENCE:?Set OIDC_AUDIENCE before deploying}"
: "${OIDC_JWKS_URL:?Set OIDC_JWKS_URL before deploying}"

IMAGE_URI="${IMAGE_URI:-${REGION}-docker.pkg.dev/${PROJECT_ID}/apollos/apollos-server:latest}"

echo "Building Rust server image: ${IMAGE_URI}"

gcloud auth configure-docker "${REGION}-docker.pkg.dev"
docker build -t "${IMAGE_URI}" .
docker push "${IMAGE_URI}"

echo "Deploying ${SERVICE_NAME} to Cloud Run..."

gcloud run deploy "${SERVICE_NAME}" \
  --image "${IMAGE_URI}" \
  --region "${REGION}" \
  --project "${PROJECT_ID}" \
  --max-instances=1 \
  --session-affinity \
  --timeout=3600 \
  --cpu=2 \
  --memory=2Gi \
  --set-env-vars="APP_ENV=production,ENABLE_GEMINI_LIVE=1,GEMINI_API_KEY=${GEMINI_API_KEY},USE_FIRESTORE=1,FIRESTORE_REQUIRED=1,GOOGLE_CLOUD_PROJECT=${PROJECT_ID},WS_AUTH_MODE=oidc_broker,OIDC_REQUIRE_STRICT=1,OIDC_ISSUER=${OIDC_ISSUER},OIDC_AUDIENCE=${OIDC_AUDIENCE},OIDC_JWKS_URL=${OIDC_JWKS_URL},TWILIO_REQUIRED=1,SINGLE_INSTANCE_ONLY=1,ENABLE_DEV_ENDPOINTS=0,CORS_ALLOW_ORIGINS=${FRONTEND_ORIGIN}"

echo "Deployment complete."
