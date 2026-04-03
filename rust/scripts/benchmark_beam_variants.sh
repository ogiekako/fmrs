#!/usr/bin/env bash
set -euo pipefail

BIN="${BIN:-./target/release/fmrs}"
PARALLEL="${PARALLEL:-24}"
DURATION="${DURATION:-20}"
GOAL="${GOAL:-9999}"
SEEDS_STR="${SEEDS:-713 13337 27182 31415 22222 32767}"
VARIANTS_STR="${VARIANTS:-baseline c2 c4 c8 w040 w040c2 w040c4 w040c8 u2w040c4}"

read -r -a SEEDS <<<"$SEEDS_STR"
read -r -a VARIANTS <<<"$VARIANTS_STR"

variant_envs() {
    local variant="$1"
    local -a envs=()

    if [[ "$variant" == baseline ]]; then
        return 0
    fi

    if [[ "$variant" =~ u([0-9]+) ]]; then
        envs+=("FMRS_BEAM_USE_MULT=${BASH_REMATCH[1]}")
    fi

    if [[ "$variant" =~ w([0-9]{3}) ]]; then
        envs+=("FMRS_BEAM_WEIGHT_EXPONENT=0.${BASH_REMATCH[1]}")
    fi

    if [[ "$variant" =~ c([0-9]+) ]]; then
        envs+=("FMRS_BEAM_SEEN_CAP=${BASH_REMATCH[1]}")
    fi

    if [[ ${#envs[@]} -eq 0 ]]; then
        echo "unknown variant: $variant" >&2
        return 1
    fi

    printf '%s\n' "${envs[*]}"
}

run_variant() {
    local variant="$1"
    local env_str
    env_str="$(variant_envs "$variant")"
    local -a env_args=()
    if [[ -n "$env_str" ]]; then
        read -r -a env_args <<<"$env_str"
    fi

    local -a steps=()
    local total=0

    for seed in "${SEEDS[@]}"; do
        local -a cmd=(env)
        if [[ ${#env_args[@]} -gt 0 ]]; then
            cmd+=("${env_args[@]}")
        fi
        cmd+=("$BIN" one-way-mate --seed "$seed" --parallel "$PARALLEL" --goal "$GOAL")

        local step
        local status
        set +e
        step="$(timeout "${DURATION}s" "${cmd[@]}" | awk 'NF{last=$1} END{print last+0}')"
        status=$?
        set -e
        if [[ $status -ne 0 && $status -ne 124 ]]; then
            return "$status"
        fi
        steps+=("$step")
        total=$((total + step))
        printf '%s\tseed=%s\tstep=%s\n' "$variant" "$seed" "$step"
    done

    local count="${#steps[@]}"
    local avg
    avg="$(awk -v total="$total" -v count="$count" 'BEGIN { printf "%.2f", total / count }')"
    mapfile -t sorted_steps < <(printf '%s\n' "${steps[@]}" | sort -n)
    local median="${sorted_steps[$((count / 2))]}"
    local min="${sorted_steps[0]}"
    local max="${sorted_steps[$((count - 1))]}"

    printf 'SUMMARY\t%s\tavg=%s\tmedian=%s\tmin=%s\tmax=%s\n' \
        "$variant" "$avg" "$median" "$min" "$max"
}

for variant in "${VARIANTS[@]}"; do
    run_variant "$variant"
done
