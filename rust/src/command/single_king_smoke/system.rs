use anyhow::Context as _;
use fmrs_core::search::backward::BackwardSearchStats;
use std::fmt;
use std::fs;

// FlatMemo: 2 memos × 4× pre-alloc overhead × 16B/slot = 128B/entry.
// NoHashMap64 deltas (memo_delta + prev_memo_delta): 2 × ~48B/entry ≈ 96B.
// Total ≈ 224B; round up to 256 for margin.
const BYTES_PER_ENTRY: usize = 256;

#[derive(Clone, Default)]
pub(super) struct ProcStatus {
    vm_rss_kib: Option<usize>,
    vm_size_kib: Option<usize>,
    threads: Option<usize>,
}

impl ProcStatus {
    pub(super) fn current() -> Self {
        let Ok(status) = fs::read_to_string("/proc/self/status") else {
            return Self::default();
        };
        let mut res = Self::default();
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                res.vm_rss_kib = parse_kib_field(value);
            } else if let Some(value) = line.strip_prefix("VmSize:") {
                res.vm_size_kib = parse_kib_field(value);
            } else if let Some(value) = line.strip_prefix("Threads:") {
                res.threads = value.trim().parse().ok();
            }
        }
        res
    }
}

impl fmt::Display for ProcStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opt = |x: Option<usize>| x.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
        write!(
            f,
            "rss={}KiB vmsize={}KiB threads={}",
            opt(self.vm_rss_kib),
            opt(self.vm_size_kib),
            opt(self.threads)
        )
    }
}

pub(super) struct SearchStatsDisplay(pub(super) BackwardSearchStats);

impl fmt::Display for SearchStatsDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0;
        write!(
            f,
            "search(step={} seen={} frontier={} prev_frontier={} memo={} prev_memo={})",
            s.step,
            s.seen_positions,
            s.positions_len,
            s.prev_positions_len,
            s.memo_len,
            s.prev_memo_len
        )
    }
}

fn parse_kib_field(value: &str) -> Option<usize> {
    value.split_whitespace().next()?.parse().ok()
}

/// Parse `--max-memo-entries`. The returned value is the per-seed cap **at
/// peak parallelism** (when all `parallel` seeds are concurrently running).
/// At runtime the per-seed cap grows as other seeds finish — see
/// `search_single_seed`'s dynamic memo budget.
pub(super) fn parse_max_memo_entries(
    value: &str,
    parallel: usize,
) -> anyhow::Result<Option<usize>> {
    match value {
        "auto" | "full" => {
            let entries = memo_entries_for_memory(parallel);
            eprintln!("{value} max_memo_entries={entries} (parallel={parallel})");
            Ok(Some(entries))
        }
        "none" => Ok(None),
        s => Ok(Some(s.parse::<usize>().context(
            "max-memo-entries must be a number, \"auto\", \"full\", or \"none\"",
        )?)),
    }
}

fn memo_entries_for_memory(divisor: usize) -> usize {
    let total_bytes = total_memory_bytes();
    // Reserve 20% for OS, frontier, positions, etc.
    let available = total_bytes * 4 / 5;
    available / divisor.max(1) / BYTES_PER_ENTRY
}

fn total_memory_bytes() -> usize {
    let Ok(content) = fs::read_to_string("/proc/meminfo") else {
        return 8 * 1024 * 1024 * 1024; // fallback 8GB
    };
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            if let Some(kb) = parse_kib_field(value) {
                return kb * 1024;
            }
        }
    }
    8 * 1024 * 1024 * 1024
}

/// Pool entry memory footprint estimate, in bytes per CandRef equivalent.
///
/// CandRef itself is 16 B (see backward.rs). Per-entry overhead in the live
/// Phase-1 shard buckets and the cross-shard candidate pool adds:
///   - `BinaryHeap<HeapEntry>` slot: 16 B + amortised vec growth (~1.5× ≈ 8 B)
///   - `NoHashSet64::seen` entry (u64 + hash table slack ≈ 1.5× → ~12 B)
///   - Phase-1 per-thread `local_outs` Vec slot (16 B + amortised growth)
///     contributes briefly but flushes per chunk, so amortised below 8 B/entry
///   - `Vec<CandRef>` final candidates pool: 16 B + ~8 B amortised growth
///
/// Rounded up to 64 B for safety. Used as the divisor when converting an
/// available-bytes budget into a `pool_factor` ceiling.
pub(super) const POOL_BYTES_PER_ENTRY: usize = 64;

/// Memory budget for adaptive pool sizing.
///
/// Captures the total memory budget for this process (a fixed fraction of
/// `MemTotal`) and exposes `available_bytes()` based on live RSS readings.
/// Backward search uses this to derive a `pool_factor` ceiling dynamically
/// each step instead of requiring the user to set `--max-candidates-pool`.
///
/// On non-Linux platforms (or if `/proc` is unavailable) the budget falls
/// back to `0`, which makes `pool_factor_ceiling` collapse to the existing
/// `--max-candidates-pool` value (or `candidates_pool_factor` if neither is
/// set) — i.e. behaviour reverts to the pre-budget static cap.
#[derive(Clone, Copy)]
pub(super) struct MemoryBudget {
    budget_bytes: usize,
}

impl MemoryBudget {
    /// Build a budget = `MemTotal × pct / 100`. Pass `pct = 0` to disable.
    pub(super) fn from_pct(pct: u32) -> Self {
        if pct == 0 {
            return Self { budget_bytes: 0 };
        }
        let total = total_memory_bytes();
        let budget_bytes = total.saturating_mul(pct as usize) / 100;
        Self { budget_bytes }
    }

    pub(super) fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// `budget − current RSS`, clamped at 0. Returns 0 if budget is disabled
    /// or RSS cannot be read.
    pub(super) fn available_bytes(&self) -> usize {
        if self.budget_bytes == 0 {
            return 0;
        }
        let Some(rss_kib) = current_vm_rss_kib() else {
            return 0;
        };
        self.budget_bytes.saturating_sub(rss_kib * 1024)
    }

    /// Compute the `pool_factor` ceiling for beam width `w` such that the
    /// resulting pool fits in *half* the remaining budget — leaving the other
    /// half for Phase-2 transient allocations, memo growth, OS slack, etc.
    /// Returns 0 if the budget is disabled (caller falls back to its static
    /// `--max-candidates-pool`).
    pub(super) fn pool_factor_ceiling(&self, w: usize) -> usize {
        if self.budget_bytes == 0 || w == 0 {
            return 0;
        }
        let available = self.available_bytes();
        let pool_budget = available / 2;
        pool_budget / (w.saturating_mul(POOL_BYTES_PER_ENTRY)).max(1)
    }
}

fn current_vm_rss_kib() -> Option<usize> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            return parse_kib_field(value);
        }
    }
    None
}
