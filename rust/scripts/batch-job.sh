#!/usr/bin/env bash
# Cloud Batch でフリート並列ジョブを実行するスクリプト
#
# Usage:
#   # 1. リリースビルドを GCS にアップロード
#   ./scripts/batch-job.sh upload
#
#   # 2. ジョブ投入 (5並列の例)
#   ./scripts/batch-job.sh submit 5
#
#   # 3. ステータス確認
#   ./scripts/batch-job.sh status [JOB_NAME]
#
#   # 4. ログ確認
#   ./scripts/batch-job.sh logs [JOB_NAME]
#
#   # 5. 結果ダウンロード
#   ./scripts/batch-job.sh pull
#
#   # 6. ジョブ削除
#   ./scripts/batch-job.sh delete JOB_NAME
#
set -euo pipefail

PROJECT="${GCP_PROJECT:-fmrs00}"
REGION="${GCP_REGION:-us-central1}"
MACHINE_TYPE="${BATCH_MACHINE:-n2d-highmem-96}"
GCS_BUCKET="${BATCH_GCS_BUCKET:-gs://fmrs-results}"
BINARY_GCS_PATH="${GCS_BUCKET}/bin/fmrs"
LOCAL_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SPOT="${BATCH_SPOT:-true}"

# Default fmrs arguments (override via BATCH_FMRS_ARGS)
FMRS_ARGS="${BATCH_FMRS_ARGS:-single-king-smoke ideal-backward --allow-white-pieces --parallel 1 --inner-parallel 60 --slack 100 --allowed-kinds pawn,lance,knight --no-dashmap}"

cmd_upload() {
  echo "Building release binary..."
  cargo build --release --manifest-path="$LOCAL_DIR/Cargo.toml"
  echo "Uploading to $BINARY_GCS_PATH ..."
  gcloud storage cp "$LOCAL_DIR/target/release/fmrs" "$BINARY_GCS_PATH"
  echo "Done."
}

cmd_submit() {
  local task_count="${1:?Usage: $0 submit N [-- FMRS_ARGS...]}"
  shift
  if [ "${1:-}" = "--" ]; then
    shift
    FMRS_ARGS="$*"
  fi
  local job_name="fmrs-$(date +%Y%m%d-%H%M%S)"
  local seed_log_gcs="${GCS_BUCKET}/seed-result-log.jsonl"
  local result_gcs_dir="${GCS_BUCKET}/jobs/${job_name}"

  echo "Submitting batch job: $job_name ($task_count tasks, $MACHINE_TYPE)"
  echo "  Binary: $BINARY_GCS_PATH"
  echo "  Results: $result_gcs_dir/"
  echo "  Args: $FMRS_ARGS --fleet-index \$INDEX --fleet-size $task_count"
  echo ""

  local provisioning_model="SPOT"
  if [ "$SPOT" != "true" ]; then
    provisioning_model="STANDARD"
  fi

  local job_json
  job_json=$(cat <<EOF
{
  "taskGroups": [
    {
      "taskSpec": {
        "runnables": [
          {
            "script": {
              "text": "#!/bin/bash\nset -euo pipefail\necho \"Task \$BATCH_TASK_INDEX / $task_count starting...\"\n\n# Download binary\ngsutil -q cp $BINARY_GCS_PATH /tmp/fmrs\nchmod +x /tmp/fmrs\n\n# Download shared seed-result-log\ngsutil -q cp $seed_log_gcs /tmp/seed-result-log.jsonl 2>/dev/null || touch /tmp/seed-result-log.jsonl\n\n# Run\n/tmp/fmrs $FMRS_ARGS \\\\\n  --fleet-index \$BATCH_TASK_INDEX --fleet-size $task_count \\\\\n  --seed-result-log /tmp/seed-result-log.jsonl\n\n# Upload results\ngsutil -q cp /tmp/seed-result-log.jsonl ${result_gcs_dir}/result-\$BATCH_TASK_INDEX.jsonl\necho \"Task \$BATCH_TASK_INDEX done.\"\n"
            }
          }
        ],
        "computeResource": {
          "cpuMilli": 96000,
          "memoryMib": 786432
        },
        "maxRunDuration": "86400s",
        "maxRetryCount": 3
      },
      "taskCount": $task_count,
      "taskCountPerNode": 1,
      "parallelism": $task_count
    }
  ],
  "allocationPolicy": {
    "instances": [
      {
        "policy": {
          "machineType": "$MACHINE_TYPE",
          "provisioningModel": "$provisioning_model"
        }
      }
    ],
    "location": {
      "allowedLocations": ["regions/$REGION"]
    }
  },
  "logsPolicy": {
    "destination": "CLOUD_LOGGING"
  }
}
EOF
)

  echo "$job_json" > /tmp/batch-job.json

  gcloud batch jobs submit "$job_name" \
    --project="$PROJECT" \
    --location="$REGION" \
    --config=/tmp/batch-job.json

  echo ""
  echo "Job submitted: $job_name"
  echo ""
  echo "Commands:"
  echo "  $0 status $job_name"
  echo "  $0 logs $job_name"
  echo "  $0 pull"
  echo "  $0 delete $job_name"
}

cmd_status() {
  local job_name="${1:-}"
  if [ -z "$job_name" ]; then
    echo "Recent jobs:"
    gcloud batch jobs list --project="$PROJECT" --location="$REGION" \
      --format="table(name.basename(),status.state,createTime)" \
      --sort-by="~createTime" --limit=10
  else
    gcloud batch jobs describe "$job_name" \
      --project="$PROJECT" --location="$REGION" \
      --format="yaml(status)"
  fi
}

cmd_logs() {
  local job_name="${1:?Usage: $0 logs JOB_NAME}"
  gcloud batch jobs describe "$job_name" \
    --project="$PROJECT" --location="$REGION" \
    --format="value(uid)" | xargs -I{} \
    gcloud logging read "resource.labels.job_uid={} AND severity>=INFO" \
      --project="$PROJECT" --limit=100 --format="value(textPayload)"
}

cmd_pull() {
  local dest="$LOCAL_DIR/target/batch-results"
  mkdir -p "$dest"
  echo "Pulling results from ${GCS_BUCKET}/jobs/ ..."
  gcloud storage cp --recursive "${GCS_BUCKET}/jobs/" "$dest/"

  # Merge all result files
  local merged="$LOCAL_DIR/target/seed-result-log-batch.jsonl"
  cat "$dest"/*/result-*.jsonl 2>/dev/null | sort -u > "$merged"
  local count
  count=$(wc -l < "$merged")
  echo "Merged: $count entries -> $merged"
}

cmd_delete() {
  local job_name="${1:?Usage: $0 delete JOB_NAME}"
  gcloud batch jobs delete "$job_name" \
    --project="$PROJECT" --location="$REGION" --quiet
  echo "Deleted: $job_name"
}

case "${1:-help}" in
  upload)  cmd_upload ;;
  submit)  shift; cmd_submit "$@" ;;
  status)  shift; cmd_status "$@" ;;
  logs)    shift; cmd_logs "$@" ;;
  pull)    cmd_pull ;;
  delete)  shift; cmd_delete "$@" ;;
  help|*)
    echo "Usage: $0 {upload|submit|status|logs|pull|delete}"
    echo ""
    echo "Commands:"
    echo "  upload       Build release binary and upload to GCS"
    echo "  submit N     Submit a batch job with N parallel tasks"
    echo "  status [JOB] List jobs or show job status"
    echo "  logs JOB     Show job logs"
    echo "  pull         Download and merge results"
    echo "  delete JOB   Delete a job"
    echo ""
    echo "Environment variables:"
    echo "  GCP_PROJECT       Project (default: fmrs00)"
    echo "  GCP_REGION        Region (default: us-central1)"
    echo "  BATCH_MACHINE     Machine type (default: n2d-highmem-96)"
    echo "  BATCH_GCS_BUCKET  GCS bucket (default: gs://fmrs-results)"
    echo "  BATCH_SPOT        Use spot VMs (default: true)"
    echo "  BATCH_FMRS_ARGS   fmrs arguments (override default)"
    ;;
esac
