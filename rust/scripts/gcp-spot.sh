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
#   run CMD  インスタンス上でコマンドをフォアグラウンド実行
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
#
# ---- 備考 ----
#
# - SPOT + STOP ポリシーなので、preempt されてもインスタンスは消えず
#   再度 `up` で復帰できる (ディスクも残る)。
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
LOCAL_DIR="${GCP_SPOT_LOCAL_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
REMOTE_DIR="${GCP_SPOT_REMOTE_DIR:-~/fmrs-rust}"
TMUX_SESSION="job"
BG_LOG="/tmp/fmrs-job.log"

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
# We need to extract the host (last-minus-one arg when command present)
# and the remote command, then delegate to gcloud compute ssh.
args=("$@")
# Skip any flags rsync passes (-o, -l, etc.) to find the host
i=0
while [ $i -lt ${#args[@]} ]; do
  case "${args[$i]}" in
    -*) i=$((i + 1)); [ $i -lt ${#args[@]} ] && i=$((i + 1)) ;;  # skip flag + value
    *)  break ;;
  esac
done
host="${args[$i]}"
remote_cmd="${args[@]:$((i + 1))}"
exec gcloud compute ssh "$host" --zone=__ZONE__ --command="$remote_cmd"
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
    --boot-disk-type=pd-ssd \
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
  local remote_cmd="cd $REMOTE_DIR && source ~/.cargo/env && $*"
  echo "Running on $INSTANCE_NAME: $*"
  # --ssh-flag=-t allocates a PTY so that Ctrl-C / SSH disconnect
  # sends SIGHUP to the remote process group, killing it cleanly.
  gcloud compute ssh "$INSTANCE_NAME" --zone="$ZONE" \
    --ssh-flag=-t --command="$remote_cmd"
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
  help|*)
    echo "Usage: $0 {up|push|run|run-bg|tail|pull|ssh|down|status|cost}"
    echo ""
    echo "Commands:"
    echo "  up       Create/start the spot instance"
    echo "  push     Sync local source to instance (excludes target/)"
    echo "  run CMD  Run a command on the instance (foreground)"
    echo "  run-bg CMD  Run a command in background (tmux)"
    echo "  tail     Tail the background job log"
    echo "  pull     Sync instance results back to local"
    echo "  ssh      Interactive SSH session"
    echo "  down     Delete the instance"
    echo "  cost     Show estimated cost and uptime"
    echo "  status   Show instance status"
    echo ""
    echo "Environment variables:"
    echo "  GCP_SPOT_INSTANCE  Instance name (default: fmrs-spot)"
    echo "  GCP_SPOT_ZONE      Zone (default: us-central1-a)"
    echo "  GCP_SPOT_MACHINE   Machine type (default: n2d-highmem-96)"
    echo "  GCP_SPOT_DISK_SIZE Disk size (default: 200GB)"
    ;;
esac
