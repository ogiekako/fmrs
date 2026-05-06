#!/usr/bin/env bash
#
# gcp-spot.sh — GCP spot instance runner for fmrs
#
# ローカルの CPU が詰将棋探索で埋まっているとき、GCP の spot インスタンス
# (96 vCPU / 768 GB RAM) を借りてジョブを走らせるためのスクリプト。
#
# 前提: gcloud CLI がインストール済みで、プロジェクト fmrs00 に認証済みであること。
#
# ---- 典型的なワークフロー ----
#
#   ./scripts/gcp-spot.sh up            # spot インスタンスを作成/起動
#   ./scripts/gcp-spot.sh push          # ソースを転送 (target/ 等は除外)
#   ./scripts/gcp-spot.sh run-bg \
#       "cargo run --release -- single-king-smoke ideal-backward --parallel 96"
#                                       # バックグラウンドで実行 (tmux)
#   ./scripts/gcp-spot.sh tail          # ログを眺める (Ctrl-C で抜ける)
#   ./scripts/gcp-spot.sh pull          # 結果 (jsonl 等) を手元に同期
#   ./scripts/gcp-spot.sh down          # インスタンスを削除
#
# ---- 全コマンド一覧 ----
#
#   up       インスタンスを作成または再起動
#   push     rsync でローカル → インスタンスへソースを転送
#   run CMD  インスタンス上でコマンドをフォアグラウンド実行 (preempt 時に自動再開)
#   run-bg CMD  tmux 経由でバックグラウンド実行
#   tail     バックグラウンドジョブのログを tail -f
#   pull     rsync でインスタンス → ローカルへ結果を同期
#   ssh      対話的に SSH 接続
#   down     インスタンスを削除
#   status   インスタンスの状態を表示
#
# ---- カスタマイズ (環境変数) ----
#
#   GCP_SPOT_INSTANCE   インスタンス名       (default: fmrs-spot)
#   GCP_SPOT_ZONE       ゾーン               (default: us-central1-a)
#   GCP_SPOT_MACHINE    マシンタイプ         (default: n2d-highmem-96)
#   GCP_SPOT_DISK_SIZE  ディスクサイズ       (default: 200GB)
#   GCP_SPOT_DISK_TYPE  ディスクタイプ       (default: pd-ssd, c4d は pd-balanced)
#
# ---- 備考 ----
#
# - SPOT + STOP ポリシーなので、preempt されてもインスタンスは消えず
#   再度 `up` で復帰できる (ディスクも残る)。
# - `run` は preempt 検知時に自動で start を試行し (stockout 時は backoff
#   付きで再試行)、SSH 復帰後にコマンドを再実行する。コマンド側が
#   checkpoint 経由で restart 可能であることが前提。
# - push/pull は rsync over `gcloud compute ssh --plain` で差分転送。
# - `run-bg` は tmux セッション "job" で実行。`ssh` してから
#   `tmux attach -t job` で直接アタッチもできる。
#
set -euo pipefail

# --- Configuration (override via environment) ---
INSTANCE_NAME="${GCP_SPOT_INSTANCE:-fmrs-spot}"
ZONE="${GCP_SPOT_ZONE:-us-central1-a}"
MACHINE_TYPE="${GCP_SPOT_MACHINE:-n2d-highmem-96}"
IMAGE_FAMILY="${GCP_SPOT_IMAGE_FAMILY:-ubuntu-2404-lts-amd64}"
IMAGE_PROJECT="${GCP_SPOT_IMAGE_PROJECT:-ubuntu-os-cloud}"
DISK_SIZE="${GCP_SPOT_DISK_SIZE:-200GB}"
DISK_TYPE="${GCP_SPOT_DISK_TYPE:-pd-ssd}"
LOCAL_DIR="${GCP_SPOT_LOCAL_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
REMOTE_DIR="${GCP_SPOT_REMOTE_DIR:-~/fmrs-rust}"
TMUX_SESSION="job"
BG_LOG="/tmp/fmrs-job.log"
GCS_BUCKET="${GCP_SPOT_GCS_BUCKET:-gs://fmrs-results}"
FLEET_PREFIX="${GCP_SPOT_FLEET_PREFIX:-fmrs-fleet}"
REGION="${ZONE%-*}"

ssh_cmd() {
  gcloud compute ssh "$INSTANCE_NAME" --zone="$ZONE" --command="$1"
}

# Wrapper script for rsync -e: gcloud compute ssh expects --command,
# but rsync invokes the transport as `ssh host command...`.
# We write a tiny helper that translates that calling convention.
rsync_ssh_wrapper() {
  local wrapper
  wrapper=$(mktemp /tmp/gcp-rsync-ssh.XXXXXX)
  cat > "$wrapper" <<'WRAPPER'
#!/usr/bin/env bash
# rsync calls: $0 [ssh-flags...] host command...
args=("$@")
i=0
while [ $i -lt ${#args[@]} ]; do
  case "${args[$i]}" in
    -*) i=$((i + 1)); [ $i -lt ${#args[@]} ] && i=$((i + 1)) ;;
    *)  break ;;
  esac
done
host="${args[$i]}"
remote_cmd="${args[@]:$((i + 1))}"
zone=$(gcloud compute instances list --filter="name=$host" --format="value(zone)" 2>/dev/null)
if [ -z "$zone" ]; then
  zone="__ZONE__"
fi
exec gcloud compute ssh "$host" --zone="$zone" --command="$remote_cmd"
WRAPPER
  sed -i "s|__ZONE__|$ZONE|g" "$wrapper"
  chmod +x "$wrapper"
  echo "$wrapper"
}

wait_for_ssh() {
  echo "Waiting for SSH to become available..."
  local retries=0
  while ! gcloud compute ssh "$INSTANCE_NAME" --zone="$ZONE" --command="true" 2>/dev/null; do
    retries=$((retries + 1))
    if [ "$retries" -ge 60 ]; then
      echo "ERROR: SSH not available after 60 attempts" >&2
      exit 1
    fi
    sleep 5
  done
  echo "SSH ready."
}

instance_status() {
  gcloud compute instances describe "$INSTANCE_NAME" --zone="$ZONE" \
    --format="value(status)" 2>/dev/null || echo "NOT_FOUND"
}

# Try `gcloud compute instances start` until success, retrying on stockout
# (ZONE_RESOURCE_POOL_EXHAUSTED) and other transient errors with backoff.
wait_for_capacity_and_start() {
  local backoff=30 max=600
  while true; do
    local status
    status=$(instance_status)
    if [ "$status" = "RUNNING" ]; then
      return 0
    fi
    local err rc=0
    err=$(gcloud compute instances start "$INSTANCE_NAME" --zone="$ZONE" 2>&1) || rc=$?
    if [ "$rc" -eq 0 ]; then
      return 0
    fi
    if echo "$err" | grep -q "ZONE_RESOURCE_POOL_EXHAUSTED"; then
      echo ">>> Stockout in $ZONE. Retrying in ${backoff}s..." >&2
    else
      echo ">>> Start failed (rc=$rc): $err" >&2
      echo ">>> Retrying in ${backoff}s..." >&2
    fi
    sleep "$backoff"
    backoff=$((backoff * 2))
    [ "$backoff" -gt "$max" ] && backoff="$max"
  done
}

provision_rust() {
  echo "Installing build essentials on instance..."
  ssh_cmd 'sudo apt-get update -qq && sudo apt-get install -y -qq build-essential pkg-config'
  echo "Checking Rust toolchain on instance..."
  if ssh_cmd "source ~/.cargo/env 2>/dev/null; which cargo" 2>/dev/null; then
    echo "Rust already installed."
    return
  fi
  echo "Installing Rust toolchain..."
  ssh_cmd 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && source ~/.cargo/env && rustc --version'
}

cmd_up() {
  local existing
  existing=$(gcloud compute instances describe "$INSTANCE_NAME" --zone="$ZONE" --format="value(status)" 2>/dev/null || true)
  if [ "$existing" = "RUNNING" ]; then
    echo "Instance $INSTANCE_NAME is already running."
    return
  elif [ "$existing" = "TERMINATED" ] || [ "$existing" = "STOPPED" ]; then
    echo "Starting existing instance $INSTANCE_NAME..."
    gcloud compute instances start "$INSTANCE_NAME" --zone="$ZONE"
    wait_for_ssh
    return
  elif [ -n "$existing" ]; then
    echo "Instance is in state: $existing. Waiting..."
    wait_for_ssh
    return
  fi

  echo "Creating spot instance $INSTANCE_NAME ($MACHINE_TYPE in $ZONE)..."
  gcloud compute instances create "$INSTANCE_NAME" \
    --zone="$ZONE" \
    --machine-type="$MACHINE_TYPE" \
    --provisioning-model=SPOT \
    --instance-termination-action=STOP \
    --image-family="$IMAGE_FAMILY" \
    --image-project="$IMAGE_PROJECT" \
    --boot-disk-size="$DISK_SIZE" \
    --boot-disk-type="$DISK_TYPE" \
    --scopes=default

  wait_for_ssh
  provision_rust

  echo ""
  echo "Instance ready. Next steps:"
  echo "  $0 push       # sync source code"
  echo "  $0 run 'cmd'  # run a command"
}

cmd_push() {
  echo "Syncing $LOCAL_DIR -> $INSTANCE_NAME:$REMOTE_DIR ..."
  ssh_cmd "mkdir -p $REMOTE_DIR"
  local wrapper
  wrapper=$(rsync_ssh_wrapper)
  trap "rm -f '$wrapper'" RETURN
  rsync -avz --progress \
    --exclude='target/' \
    --exclude='.git/' \
    -e "$wrapper" \
    "$LOCAL_DIR/" "$INSTANCE_NAME":"$REMOTE_DIR/"
  echo "Push complete."
}

cmd_pull() {
  echo "Syncing $INSTANCE_NAME:$REMOTE_DIR -> $LOCAL_DIR ..."
  local wrapper
  wrapper=$(rsync_ssh_wrapper)
  trap "rm -f '$wrapper'" RETURN
  rsync -avz --progress \
    --exclude='target/' \
    --exclude='.git/' \
    -e "$wrapper" \
    "$INSTANCE_NAME":"$REMOTE_DIR/" "$LOCAL_DIR/"
  echo "Pull complete."
}

cmd_run() {
  if [ $# -eq 0 ]; then
    echo "Usage: $0 run 'command'" >&2
    exit 1
  fi
  local user_cmd="$*"
  local remote_cmd="cd $REMOTE_DIR && source ~/.cargo/env && $user_cmd"
  trap 'echo ">>> Interrupted. Exiting." >&2; exit 130' INT
  while true; do
    echo ">>> Running on $INSTANCE_NAME: $user_cmd"
    local rc=0
    local supervisor_pid=$$

    # Watchdog: gcloud ssh's TCP connection often hangs after preempt
    # rather than returning an error. Poll the instance status and SIGTERM
    # the ssh subprocess if the instance is no longer RUNNING.
    (
      while sleep 20; do
        kill -0 "$supervisor_pid" 2>/dev/null || exit 0
        local s
        s=$(gcloud compute instances describe "$INSTANCE_NAME" --zone="$ZONE" \
              --format="value(status)" 2>/dev/null || echo "UNKNOWN")
        if [ "$s" != "RUNNING" ]; then
          for child in $(pgrep -P "$supervisor_pid" 2>/dev/null); do
            [ "$child" = "$BASHPID" ] && continue
            kill -TERM "$child" 2>/dev/null || true
          done
          exit 0
        fi
      done
    ) &
    local watchdog_pid=$!

    # --ssh-flag=-t allocates a PTY so that Ctrl-C / SSH disconnect
    # sends SIGHUP to the remote process group. ServerAlive* makes SSH
    # detect a dead TCP connection within ~60s as a backup to the watchdog.
    gcloud compute ssh "$INSTANCE_NAME" --zone="$ZONE" \
      --ssh-flag=-t \
      --ssh-flag=-oServerAliveInterval=15 \
      --ssh-flag=-oServerAliveCountMax=4 \
      --command="$remote_cmd" || rc=$?

    kill -TERM "$watchdog_pid" 2>/dev/null || true
    wait "$watchdog_pid" 2>/dev/null || true

    local status
    status=$(instance_status)
    case "$status" in
      RUNNING)
        # Command finished (success or failure) without preemption.
        exit "$rc"
        ;;
      TERMINATED|STOPPING|STOPPED|PROVISIONING|STAGING|REPAIRING)
        echo ">>> Instance not RUNNING (status=$status, rc=$rc). Auto-recovering..." >&2
        wait_for_capacity_and_start
        wait_for_ssh
        echo ">>> Resumed. Re-running command (assumes restart-safe)." >&2
        ;;
      *)
        echo ">>> Unexpected instance status=$status, rc=$rc. Aborting." >&2
        exit "$rc"
        ;;
    esac
  done
}

cmd_run_bg() {
  if [ $# -eq 0 ]; then
    echo "Usage: $0 run-bg 'command'" >&2
    exit 1
  fi
  local remote_cmd="cd $REMOTE_DIR && source ~/.cargo/env && $*"
  echo "Running in background on $INSTANCE_NAME: $*"
  ssh_cmd "tmux kill-session -t $TMUX_SESSION 2>/dev/null || true; tmux new-session -d -s $TMUX_SESSION 'bash -c \"($remote_cmd) 2>&1 | tee $BG_LOG; echo; echo === DONE exit=\\\$? ===\"'"
  echo "Job started in tmux session '$TMUX_SESSION'."
  echo "  $0 tail    # watch output"
  echo "  $0 ssh     # then: tmux attach -t $TMUX_SESSION"
}

cmd_tail() {
  echo "Tailing $INSTANCE_NAME:$BG_LOG (Ctrl-C to detach)..."
  ssh_cmd "tail -f $BG_LOG"
}

cmd_ssh() {
  gcloud compute ssh "$INSTANCE_NAME" --zone="$ZONE"
}

cmd_down() {
  echo "Deleting instance $INSTANCE_NAME..."
  gcloud compute instances delete "$INSTANCE_NAME" --zone="$ZONE" --quiet
  echo "Done."
}

cmd_status() {
  gcloud compute instances describe "$INSTANCE_NAME" --zone="$ZONE" \
    --format="table(name,status,machineType.basename(),scheduling.provisioningModel,networkInterfaces[0].accessConfigs[0].natIP)" 2>/dev/null || echo "Instance $INSTANCE_NAME not found."
}

cmd_cost() {
  # --- Spot pricing for n2d (us-central1, as of 2025) ---
  # Source: https://cloud.google.com/compute/vm-instance-pricing
  # These are approximate; actual billing may differ slightly.
  local -A SPOT_VCPU_RATE  # $/vCPU/hr
  local -A SPOT_RAM_RATE   # $/GB/hr
  SPOT_VCPU_RATE[n2d]=0.004855
  SPOT_RAM_RATE[n2d]=0.000671
  SPOT_VCPU_RATE[n2]=0.006655
  SPOT_RAM_RATE[n2]=0.000892
  SPOT_VCPU_RATE[c3]=0.007180
  SPOT_RAM_RATE[c3]=0.000962
  SPOT_VCPU_RATE[c3d]=0.005765
  SPOT_RAM_RATE[c3d]=0.000773
  SPOT_VCPU_RATE[c4]=0.007735
  SPOT_RAM_RATE[c4]=0.001036
  SPOT_VCPU_RATE[c4d]=0.005440
  SPOT_RAM_RATE[c4d]=0.000729

  local instance_json
  instance_json=$(gcloud compute instances describe "$INSTANCE_NAME" --zone="$ZONE" --format=json 2>/dev/null) || {
    echo "Instance $INSTANCE_NAME not found."
    return 1
  }

  local status machine_type_url machine_type last_start
  status=$(echo "$instance_json" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])")
  machine_type_url=$(echo "$instance_json" | python3 -c "import sys,json; print(json.load(sys.stdin)['machineType'])")
  machine_type=$(basename "$machine_type_url")
  last_start=$(echo "$instance_json" | python3 -c "import sys,json; print(json.load(sys.stdin).get('lastStartTimestamp',''))")

  local cpus mem_mb
  local mt_json
  mt_json=$(gcloud compute machine-types describe "$machine_type" --zone="$ZONE" --format=json 2>/dev/null)
  cpus=$(echo "$mt_json" | python3 -c "import sys,json; print(json.load(sys.stdin)['guestCpus'])")
  mem_mb=$(echo "$mt_json" | python3 -c "import sys,json; print(json.load(sys.stdin)['memoryMb'])")

  python3 -c "
import sys, json
from datetime import datetime, timezone

status = '$status'
machine_type = '$machine_type'
last_start = '$last_start'
cpus = int('$cpus')
mem_mb = int('$mem_mb')
mem_gb = mem_mb / 1024

# Detect machine family (e.g. 'n2d' from 'n2d-highmem-96')
family = '-'.join(machine_type.split('-')[:-1])
# strip the tier suffix (highmem, standard, etc.)
parts = machine_type.split('-')
family_key = parts[0] if len(parts) >= 2 else machine_type
# Try with first part, then first two parts for families like c3d
spot_vcpu_rates = {
    'n2d': 0.004855, 'n2': 0.006655, 'c3': 0.007180,
    'c3d': 0.005765, 'c4': 0.007735, 'c4d': 0.005440,
    'n1': 0.006655, 'e2': 0.006655,
}
spot_ram_rates = {
    'n2d': 0.000671, 'n2': 0.000892, 'c3': 0.000962,
    'c3d': 0.000773, 'c4': 0.001036, 'c4d': 0.000729,
    'n1': 0.000892, 'e2': 0.000892,
}

# Try 'n2d' first, then 'n2'
vcpu_rate = spot_vcpu_rates.get(parts[0], spot_vcpu_rates.get(parts[0][:-1], 0.006))
ram_rate = spot_ram_rates.get(parts[0], spot_ram_rates.get(parts[0][:-1], 0.0008))

hourly_usd = cpus * vcpu_rate + mem_gb * ram_rate
jpy_rate = 145  # approximate USD/JPY

print(f'Instance:     {machine_type} ({cpus} vCPU, {mem_gb:.0f} GB RAM)')
print(f'Status:       {status}')
print(f'Spot rate:    \${hourly_usd:.3f}/hr  (~{hourly_usd * jpy_rate:.0f} JPY/hr)')
print(f'              \${hourly_usd * 24:.2f}/day  (~{hourly_usd * 24 * jpy_rate:.0f} JPY/day)')
print()

if last_start and status == 'RUNNING':
    # Parse ISO timestamp
    start = datetime.fromisoformat(last_start)
    now = datetime.now(timezone.utc)
    elapsed = now - start
    hours = elapsed.total_seconds() / 3600
    cost_usd = hours * hourly_usd
    cost_jpy = cost_usd * jpy_rate

    days = int(hours // 24)
    remaining_hours = hours % 24
    if days > 0:
        elapsed_str = f'{days}d {remaining_hours:.1f}h'
    else:
        elapsed_str = f'{hours:.1f}h'

    print(f'Uptime:       {elapsed_str} (since {start.strftime(\"%Y-%m-%d %H:%M %Z\")})')
    print(f'Est. cost:    \${cost_usd:.2f}  (~{cost_jpy:.0f} JPY)')
    print()
    print(f'If stopped now:  \${cost_usd:.2f}')
    print(f'+1h:             \${cost_usd + hourly_usd:.2f}')
    print(f'+6h:             \${cost_usd + hourly_usd*6:.2f}')
    print(f'+24h:            \${cost_usd + hourly_usd*24:.2f}')
elif status in ('TERMINATED', 'STOPPED'):
    print('Instance is stopped. No compute charges accruing.')
    print('(Disk storage charges still apply: ~\$0.17/GB/month for pd-ssd)')
else:
    print(f'Last start: {last_start or \"unknown\"}')

print()
print('Note: spot prices are approximate (us-central1, 2025 rates).')
print('      Actual billing: https://console.cloud.google.com/billing')
"
}

# ============================================================
# Fleet commands — manage N identical spot instances
# ============================================================

fleet_instance_name() {
  echo "${FLEET_PREFIX}-$1"
}

fleet_list_instances() {
  gcloud compute instances list \
    --filter="name~'^${FLEET_PREFIX}-[0-9]+$' AND zone:${REGION}" \
    --format="value(name)" 2>/dev/null | sort -t- -k3 -n
}

fleet_instance_zone() {
  gcloud compute instances list \
    --filter="name=$1" \
    --format="value(zone)" 2>/dev/null
}

fleet_ssh() {
  local name="$1"; shift
  local z
  z=$(fleet_instance_zone "$name")
  if [ -z "$z" ]; then
    echo "ERROR: cannot find zone for $name" >&2
    return 1
  fi
  gcloud compute ssh "$name" --zone="$z" "$@"
}

fleet_instance_count() {
  fleet_list_instances | wc -l
}

cmd_fleet_up() {
  local count="${1:?Usage: $0 fleet-up N}"
  echo "Creating fleet of $count instances ($MACHINE_TYPE in $REGION)..."

  for i in $(seq 0 $((count - 1))); do
    local name
    name=$(fleet_instance_name "$i")
    local existing_zone
    existing_zone=$(fleet_instance_zone "$name")
    local existing=""
    if [ -n "$existing_zone" ]; then
      existing=$(gcloud compute instances describe "$name" --zone="$existing_zone" --format="value(status)" 2>/dev/null || true)
    fi
    if [ "$existing" = "RUNNING" ]; then
      echo "  $name: already running ($existing_zone)"
    elif [ "$existing" = "TERMINATED" ] || [ "$existing" = "STOPPED" ]; then
      echo "  $name: starting... ($existing_zone)"
      gcloud compute instances start "$name" --zone="$existing_zone" &
    elif [ -z "$existing" ]; then
      echo "  $name: creating..."
      (
        local zones
        zones=$(gcloud compute zones list --filter="region:$REGION" --format="value(name)" 2>/dev/null)
        zones="$ZONE $(echo "$zones" | grep -v "^${ZONE}$")"
        local created=false
        for try_zone in $zones; do
          if gcloud compute instances create "$name" \
            --zone="$try_zone" \
            --machine-type="$MACHINE_TYPE" \
            --provisioning-model=SPOT \
            --instance-termination-action=STOP \
            --image-family="$IMAGE_FAMILY" \
            --image-project="$IMAGE_PROJECT" \
            --boot-disk-size="$DISK_SIZE" \
            --boot-disk-type="$DISK_TYPE" \
            --scopes=default,storage-rw 2>&1; then
            echo "  $name: created in $try_zone"
            created=true
            break
          else
            echo "  $name: stockout in $try_zone, trying next..." >&2
          fi
        done
        if [ "$created" = false ]; then
          echo "  ERROR: $name: no capacity in any zone in $REGION" >&2
          exit 1
        fi
      ) &
    else
      echo "  $name: state=$existing, skipping"
    fi
  done
  wait
  echo "Waiting for SSH on all instances..."
  for i in $(seq 0 $((count - 1))); do
    local name
    name=$(fleet_instance_name "$i")
    (
      local retries=0
      while ! fleet_ssh "$name" --command="true" 2>/dev/null; do
        retries=$((retries + 1))
        if [ "$retries" -ge 60 ]; then
          echo "  ERROR: $name SSH not ready after 60 retries" >&2
          exit 1
        fi
        sleep 5
      done
      echo "  $name: SSH ready"
    ) &
  done
  wait

  echo "Provisioning Rust on new instances..."
  for i in $(seq 0 $((count - 1))); do
    local name
    name=$(fleet_instance_name "$i")
    (
      if fleet_ssh "$name" --command="source ~/.cargo/env 2>/dev/null; which cargo" 2>/dev/null; then
        :
      else
        echo "  $name: installing Rust..."
        fleet_ssh "$name" --command='sudo apt-get update -qq && sudo apt-get install -y -qq build-essential pkg-config && curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
      fi
    ) &
  done
  wait
  echo "Fleet ready: $count instances."
}

cmd_fleet_push() {
  local instances
  instances=$(fleet_list_instances)
  if [ -z "$instances" ]; then
    echo "No fleet instances found." >&2
    exit 1
  fi
  local wrapper
  wrapper=$(rsync_ssh_wrapper)
  trap "rm -f '$wrapper'" RETURN

  for name in $instances; do
    echo "  Pushing to $name..."
    (
      fleet_ssh "$name" --command="mkdir -p $REMOTE_DIR" 2>/dev/null
      rsync -az \
        --exclude='target/' \
        --exclude='.git/' \
        -e "$wrapper" \
        "$LOCAL_DIR/" "$name":"$REMOTE_DIR/"
      echo "  $name: done"
    ) &
  done
  wait
  echo "Push complete."
}

cmd_fleet_run() {
  if [ $# -eq 0 ]; then
    echo "Usage: $0 fleet-run 'command with {ID}/{SIZE} placeholders'" >&2
    echo "" >&2
    echo "  {ID} is replaced with the instance index (0, 1, 2, ...)." >&2
    echo "  {SIZE} is replaced with the total number of instances." >&2
    echo "  The shared seed-result-log is synced from/to GCS automatically." >&2
    exit 1
  fi
  local user_cmd="$*"
  local instances
  instances=$(fleet_list_instances)
  if [ -z "$instances" ]; then
    echo "No fleet instances found." >&2
    exit 1
  fi

  local gcs_log="${GCS_BUCKET}/seed-result-log.jsonl"
  local local_log="seed-result-log.jsonl"

  echo "Fleet run: $user_cmd"
  echo "GCS shared log: $gcs_log"
  echo ""

  local fleet_size
  fleet_size=$(echo "$instances" | wc -w)

  local idx=0
  for name in $instances; do
    local instance_cmd="${user_cmd//\{ID\}/$idx}"
    instance_cmd="${instance_cmd//\{SIZE\}/$fleet_size}"

    # Wrapper: sync log from GCS, run command, sync log back to GCS periodically
    local wrapped_cmd
    wrapped_cmd=$(cat <<RUNCMD
cd $REMOTE_DIR && source ~/.cargo/env
# Sync shared results from GCS at startup
gsutil -q cp "$gcs_log" "$local_log" 2>/dev/null || touch "$local_log"
echo "[$name] Starting: $instance_cmd"
# Run the command, periodically uploading results
(
  while sleep 60; do
    gsutil -q cp "$local_log" "${GCS_BUCKET}/${name}-result.jsonl" 2>/dev/null
  done
) &
SYNC_PID=\$!
$instance_cmd
EXIT_CODE=\$?
kill \$SYNC_PID 2>/dev/null
# Final upload
gsutil -q cp "$local_log" "${GCS_BUCKET}/${name}-result.jsonl" 2>/dev/null
echo "[$name] Done (exit=\$EXIT_CODE)"
exit \$EXIT_CODE
RUNCMD
)

    echo "  $name: starting (id=$idx)..."
    fleet_ssh "$name" \
      --command="tmux kill-session -t $TMUX_SESSION 2>/dev/null || true; tmux new-session -d -s $TMUX_SESSION 'bash -c \"($wrapped_cmd) 2>&1 | tee $BG_LOG; echo === DONE ===\" '"
    idx=$((idx + 1))
  done
  echo ""
  echo "All instances started. Use:"
  echo "  $0 fleet-tail     # watch merged output"
  echo "  $0 fleet-status   # check status"
  echo "  $0 fleet-pull     # download results"
}

cmd_fleet_tail() {
  local instances
  instances=$(fleet_list_instances)
  if [ -z "$instances" ]; then
    echo "No fleet instances found." >&2
    exit 1
  fi

  echo "Tailing all fleet instances (Ctrl-C to stop)..."
  echo "---"

  for name in $instances; do
    (
      fleet_ssh "$name" \
        --command="tail -f $BG_LOG 2>/dev/null" 2>/dev/null | \
        sed "s/^/[$name] /"
    ) &
  done
  trap 'kill $(jobs -p) 2>/dev/null; exit 0' INT
  wait
}

cmd_fleet_pull() {
  local instances
  instances=$(fleet_list_instances)
  if [ -z "$instances" ]; then
    echo "No fleet instances found." >&2
    exit 1
  fi

  local merged="seed-result-log-merged.jsonl"
  > "$LOCAL_DIR/$merged"

  echo "Pulling results from fleet..."
  for name in $instances; do
    local local_file="$LOCAL_DIR/seed-result-${name}.jsonl"
    (
      local wrapper
      wrapper=$(rsync_ssh_wrapper)
      rsync -az -e "$wrapper" \
        "$name":"$REMOTE_DIR/seed-result-log.jsonl" "$local_file" 2>/dev/null && \
        echo "  $name: pulled" || echo "  $name: no results yet"
      rm -f "$wrapper"
    ) &
  done
  wait

  # Merge all result logs (dedup by seed index)
  cat "$LOCAL_DIR"/seed-result-${FLEET_PREFIX}-*.jsonl 2>/dev/null | \
    sort -u > "$LOCAL_DIR/$merged"
  local count
  count=$(wc -l < "$LOCAL_DIR/$merged")
  echo "Merged results: $count entries -> $LOCAL_DIR/$merged"

  # Upload merged to GCS for future runs
  gsutil -q cp "$LOCAL_DIR/$merged" "${GCS_BUCKET}/seed-result-log.jsonl" 2>/dev/null && \
    echo "Uploaded merged log to GCS." || echo "Warning: GCS upload failed."
}

cmd_fleet_status() {
  local instances
  instances=$(fleet_list_instances)
  if [ -z "$instances" ]; then
    echo "No fleet instances found."
    return
  fi
  echo "Fleet instances (prefix=$FLEET_PREFIX):"
  gcloud compute instances list \
    --filter="name~'^${FLEET_PREFIX}-[0-9]+$' AND zone:${REGION}" \
    --format="table(name,zone.basename(),status,machineType.basename(),networkInterfaces[0].accessConfigs[0].natIP)"
}

cmd_fleet_down() {
  local instances
  instances=$(fleet_list_instances)
  if [ -z "$instances" ]; then
    echo "No fleet instances found."
    return
  fi
  echo "Deleting fleet instances: $(echo $instances | tr '\n' ' ')"
  for name in $instances; do
    (
      local z
      z=$(fleet_instance_zone "$name")
      if [ -n "$z" ]; then
        gcloud compute instances delete "$name" --zone="$z" --quiet
      fi
    ) &
  done
  wait
  echo "Fleet deleted."
}

cmd_fleet_cost() {
  local instances
  instances=$(fleet_list_instances)
  if [ -z "$instances" ]; then
    echo "No fleet instances found."
    return
  fi
  local total_hourly=0
  local running_count=0
  for name in $instances; do
    local status
    local z
    z=$(fleet_instance_zone "$name")
    status=$(gcloud compute instances describe "$name" --zone="$z" --format="value(status)" 2>/dev/null || echo "UNKNOWN")
    if [ "$status" = "RUNNING" ]; then
      running_count=$((running_count + 1))
    fi
  done
  # Use single-instance cost logic with the fleet count
  local INSTANCE_NAME_BAK="$INSTANCE_NAME"
  INSTANCE_NAME=$(fleet_instance_name 0)
  echo "Fleet: $(echo $instances | wc -w) instances, $running_count running"
  echo ""
  if [ "$running_count" -gt 0 ]; then
    echo "Per-instance cost:"
    cmd_cost 2>/dev/null | head -3
    echo ""
    # Extract hourly rate and multiply
    local hourly
    hourly=$(cmd_cost 2>/dev/null | grep "Spot rate" | grep -oP '\$[\d.]+/hr' | tr -d '$/hr')
    if [ -n "$hourly" ]; then
      echo "Fleet total (${running_count} running):"
      echo "  \$$(echo "$hourly * $running_count" | bc)/hr"
      echo "  \$$(echo "$hourly * $running_count * 24" | bc)/day"
    fi
  fi
  INSTANCE_NAME="$INSTANCE_NAME_BAK"
}

# --- Main dispatch ---
case "${1:-help}" in
  up)       cmd_up ;;
  push)     shift; cmd_push "$@" ;;
  pull)     cmd_pull ;;
  run)      shift; cmd_run "$@" ;;
  run-bg)   shift; cmd_run_bg "$@" ;;
  tail)     cmd_tail ;;
  ssh)      cmd_ssh ;;
  down)     cmd_down ;;
  cost)     cmd_cost ;;
  status)   cmd_status ;;
  fleet-up)     shift; cmd_fleet_up "$@" ;;
  fleet-push)   cmd_fleet_push ;;
  fleet-run)    shift; cmd_fleet_run "$@" ;;
  fleet-tail)   cmd_fleet_tail ;;
  fleet-pull)   cmd_fleet_pull ;;
  fleet-down)   cmd_fleet_down ;;
  fleet-status) cmd_fleet_status ;;
  fleet-cost)   cmd_fleet_cost ;;
  help|*)
    echo "Usage: $0 {up|push|run|run-bg|tail|pull|ssh|down|status|cost}"
    echo "       $0 {fleet-up|fleet-push|fleet-run|fleet-tail|fleet-pull|fleet-down|fleet-status|fleet-cost}"
    echo ""
    echo "Single-instance commands:"
    echo "  up       Create/start the spot instance"
    echo "  push     Sync local source to instance (excludes target/)"
    echo "  run CMD  Run a command on the instance (foreground, auto-restart on preempt)"
    echo "  run-bg CMD  Run a command in background (tmux)"
    echo "  tail     Tail the background job log"
    echo "  pull     Sync instance results back to local"
    echo "  ssh      Interactive SSH session"
    echo "  down     Delete the instance"
    echo "  cost     Show estimated cost and uptime"
    echo "  status   Show instance status"
    echo ""
    echo "Fleet commands:"
    echo "  fleet-up N       Create N spot instances"
    echo "  fleet-push       Push source to all fleet instances"
    echo "  fleet-run CMD    Run CMD on all instances ({ID} = index, {SIZE} = total)"
    echo "  fleet-tail       Tail merged output from all instances"
    echo "  fleet-pull       Pull and merge results from all instances"
    echo "  fleet-down       Delete all fleet instances"
    echo "  fleet-status     Show all fleet instance statuses"
    echo "  fleet-cost       Show estimated fleet cost"
    echo ""
    echo "Environment variables:"
    echo "  GCP_SPOT_INSTANCE      Instance name (default: fmrs-spot)"
    echo "  GCP_SPOT_ZONE          Zone (default: us-central1-a)"
    echo "  GCP_SPOT_MACHINE       Machine type (default: n2d-highmem-96)"
    echo "  GCP_SPOT_DISK_SIZE     Disk size (default: 200GB)"
    echo "  GCP_SPOT_DISK_TYPE     Disk type (default: pd-ssd)"
    echo "  GCP_SPOT_GCS_BUCKET    GCS bucket for shared results (default: gs://fmrs-results)"
    echo "  GCP_SPOT_FLEET_PREFIX  Fleet instance name prefix (default: fmrs-fleet)"
    ;;
esac
