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

pub(super) fn parse_max_memo_entries(
    value: &str,
    parallel: usize,
    inner_parallel: usize,
) -> anyhow::Result<Option<usize>> {
    match value {
        "auto" => {
            let total_cores = default_parallelism();
            // by_cores: divide budget across the maximum concurrent seeds the
            // machine can run (each seed uses inner_parallel cores).
            let by_cores = memo_entries_for_memory(total_cores / inner_parallel.max(1));
            // by_parallel: divide budget among actually concurrent seeds.
            let by_parallel = memo_entries_for_memory(parallel);
            let entries = by_cores.min(by_parallel);
            eprintln!(
                "auto max_memo_entries={entries} (parallel={parallel} inner_parallel={inner_parallel} total_cores={total_cores} by_cores={by_cores} by_parallel={by_parallel})"
            );
            Ok(Some(entries))
        }
        "full" => {
            let entries = memo_entries_for_memory(parallel);
            eprintln!(
                "full max_memo_entries={entries} (parallel={parallel} inner_parallel={inner_parallel})"
            );
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

fn default_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
