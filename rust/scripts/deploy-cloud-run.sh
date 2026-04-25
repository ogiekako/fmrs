#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="${SERVICE_NAME:-fmrs-api}"
REGION="${REGION:-asia-northeast1}"
ALLOWED_ORIGINS="${ALLOWED_ORIGINS:-https://ogiekako.github.io}"
MEMORY="${MEMORY:-8Gi}"
CPU="${CPU:-2}"
TIMEOUT="${TIMEOUT:-3600}"
CONCURRENCY="${CONCURRENCY:-1}"

if ! command -v gcloud >/dev/null 2>&1; then
  echo "gcloud が見つかりません。https://cloud.google.com/sdk/docs/install を参照してください。" >&2
  exit 1
fi

PROJECT_ID="$(gcloud config get-value project 2>/dev/null || true)"
if [[ -z "${PROJECT_ID}" || "${PROJECT_ID}" == "(unset)" ]]; then
  echo "gcloud の project が未設定です。gcloud config set project <PROJECT_ID> を実行してください。" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "Deploying ${SERVICE_NAME} to Cloud Run in ${REGION} (project: ${PROJECT_ID})"
gcloud run deploy "${SERVICE_NAME}" \
  --source "${ROOT}" \
  --region "${REGION}" \
  --allow-unauthenticated \
  --memory "${MEMORY}" \
  --cpu "${CPU}" \
  --concurrency "${CONCURRENCY}" \
  --timeout "${TIMEOUT}" \
  --min-instances 0 \
  --max-instances 1 \
  --set-env-vars "FMRS_ALLOWED_ORIGINS=${ALLOWED_ORIGINS}"
