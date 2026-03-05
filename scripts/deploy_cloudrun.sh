#!/usr/bin/env bash

# Example deployment script for Google Cloud Run
set -e

PROJECT_ID="your_gcp_project_id"
REGION="us-central1"
SERVICE_NAME="apollos-backend"

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
  --use-http2 \
  --session-affinity \
  --timeout=3600 \
  --cpu=2 \
  --memory=2Gi \
  --set-env-vars="ENABLE_GEMINI_LIVE=1,GEMINI_MODEL=gemini-live-2.5-flash-native-audio" \
  --set-secrets="GEMINI_API_KEY=your_secret_name:latest"

echo "✅ Deployment complete!"
