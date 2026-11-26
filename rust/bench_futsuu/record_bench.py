#!/usr/bin/env python3
"""Record futsuu_bench JSON runs (1T deterministic std suite + MT parallel long suite)
into history.json, render SVG charts + README.md for direct viewing on GitHub,
and emit a Markdown summary.

Significance criteria (when --only-if-significant is set):
  - First recorded entry in history
  - Any problem's mate verdict or mate step count changed (ignoring redundant MT step drift)
  - Any node count changed in 1T deterministic std suite (0% CI jitter)
  - 1T total time or aggregate kNPS changed by >= --threshold-pct (default: 3.0%)
  - MT long suite thread count changed, or total time/nodes/kNPS changed by >= --long-threshold-pct (default: 5.0%)
  - --force flag is passed (e.g. workflow_dispatch or commit message has [bench])
"""

import argparse
import datetime
import html
import json
import os
import sys


def pct_delta(new: float, old: float) -> float:
    if old == 0:
        return 0.0
    return round((new - old) / old * 100.0, 2)


def fmt_compact(v: float, unit: str = "") -> str:
    if unit == "s":
        if v < 1.0:
            return f"{v:.3f}s"
        if v < 10.0:
            return f"{v:.2f}s"
        return f"{v:.1f}s"
    if unit == "nodes":
        if v >= 1_000_000:
            return f"{v / 1_000_000:.2f}M"
        if v >= 1_000:
            return f"{v / 1_000:.1f}k"
        return f"{v:.0f}"
    if unit == "kNPS":
        return f"{v:.0f}" if v >= 100 else f"{v:.1f}"
    return f"{v:.2f}" if v < 20 else f"{v:.0f}"


def fmt_tick(v: float, y_max: float, unit: str = "") -> str:
    if unit == "nodes":
        if y_max >= 1_000_000:
            return f"{v / 1_000_000:.1f}M"
        if y_max >= 1_000:
            return f"{v / 1_000:.0f}k"
        return f"{v:.0f}"
    if unit == "s":
        if y_max < 2.0:
            return f"{v:.2f}"
        if y_max < 20.0:
            return f"{v:.1f}"
        return f"{v:.0f}"
    return f"{v:.1f}" if y_max < 20 else f"{v:.0f}"


def fmt_mate(m: dict | None) -> str:
    if not m:
        return "-"
    if m.get("mate") is True:
        return f"M{m['steps']}" + ("(余)" if m.get("redundant") else "")
    if m.get("mate") is False:
        return "NoMate"
    return "Timeout"


def evaluate_significance(
    new_entry: dict,
    prev_entry: dict | None,
    threshold_pct: float,
    long_threshold_pct: float,
) -> tuple[bool, list[str]]:
    if prev_entry is None:
        return True, ["initial baseline"]

    reasons: list[str] = []

    # 1. 1T deterministic std suite
    cur_std = new_entry.get("std_1t")
    prv_std = prev_entry.get("std_1t")
    if cur_std and not prv_std:
        reasons.append("added std_1t suite")
    elif cur_std and prv_std:
        prev_map = {r["name"]: r for r in prv_std.get("results", [])}
        for r in cur_std.get("results", []):
            pr = prev_map.get(r["name"])
            if pr is None:
                reasons.append(f"std added {r['name']}")
                continue
            for bk in ("shtsume", "fmrs"):
                if bk in r and bk in pr:
                    if r[bk]["mate"] != pr[bk]["mate"] or r[bk]["steps"] != pr[bk]["steps"]:
                        reasons.append(f"std:{r['name']}/{bk} verdict changed")

        for key, label in [
            ("total_shtsume_nodes", "1T SH nodes"),
            ("total_fmrs_nodes", "1T FMRS nodes"),
        ]:
            cur_v = cur_std.get(key, 0)
            prv_v = prv_std.get(key, 0)
            if cur_v != prv_v:
                d = pct_delta(cur_v, prv_v)
                reasons.append(f"{label} {d:+.2f}%")

        for key, label in [
            ("total_shtsume_secs", "1T SH time"),
            ("total_fmrs_secs", "1T FMRS time"),
            ("agg_shtsume_knps", "1T SH kNPS"),
            ("agg_fmrs_knps", "1T FMRS kNPS"),
        ]:
            d = pct_delta(cur_std.get(key, 0.0), prv_std.get(key, 0.0))
            if abs(d) >= threshold_pct:
                reasons.append(f"{label} {d:+.2f}%")

    # 2. MT parallel long suite (FmrsPos)
    cur_long = new_entry.get("long_mt")
    prv_long = prev_entry.get("long_mt")
    if cur_long and not prv_long:
        reasons.append("added long_mt suite")
    elif cur_long and prv_long:
        if cur_long.get("threads") != prv_long.get("threads"):
            reasons.append(
                f"long threads {prv_long.get('threads')} -> {cur_long.get('threads')}"
            )
        prev_lmap = {r["name"]: r for r in prv_long.get("results", [])}
        for r in cur_long.get("results", []):
            pr = prev_lmap.get(r["name"])
            if pr is None:
                reasons.append(f"long added {r['name']}")
                continue
            fm = r.get("fmrs")
            pfm = pr.get("fmrs")
            if fm and pfm:
                both_red = fm.get("redundant") and pfm.get("redundant")
                steps_changed = (fm["steps"] != pfm["steps"]) and not both_red
                if fm["mate"] != pfm["mate"] or steps_changed:
                    reasons.append(f"long:{r['name']} verdict changed")

        for key, label in [
            ("total_fmrs_secs", "Long MT time"),
            ("total_fmrs_nodes", "Long MT nodes"),
            ("agg_fmrs_knps", "Long MT kNPS"),
        ]:
            d = pct_delta(cur_long.get(key, 0.0), prv_long.get(key, 0.0))
            if abs(d) >= long_threshold_pct:
                reasons.append(f"{label} {d:+.2f}%")

    return len(reasons) > 0, reasons


def render_svg_line_chart(
    title: str,
    y_label: str,
    labels: list[str],
    series: list[tuple[str, str, str, list[float | None]]],
    unit: str = "",
    annotations: list[tuple[int, str]] | None = None,
    width: int = 880,
    height: int = 310,
) -> str:
    """Render a self-contained SVG line chart.

    Each series tuple is (name, color, dasharray, values).
    """
    ml, mr, mt, mb = 74, 32, 48, 52
    pw = width - ml - mr
    ph = height - mt - mb

    vals = [v for _, _, _, ys in series for v in ys if v is not None]
    y_max = max(vals) if vals else 1.0
    if y_max <= 0:
        y_max = 1.0
    y_max *= 1.15

    n = len(labels)

    def x_pos(i: int) -> float:
        if n <= 1:
            return ml + pw / 2.0
        return ml + (i / (n - 1)) * pw

    def y_pos(v: float) -> float:
        return mt + ph - (v / y_max) * ph

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" rx="8" fill="#0f172a"/>',
        f'<text x="{ml}" y="26" fill="#e2e8f0" font-family="sans-serif" font-size="14" font-weight="600">{html.escape(title)}</text>',
    ]

    # Legend (top right)
    lx = width - mr
    for name, color, dash, _ in reversed(series):
        text_w = len(name) * 6.8 + 26
        lx -= text_w
        dash_attr = f' stroke-dasharray="{dash}"' if dash else ""
        out.append(
            f'<line x1="{lx:.1f}" y1="21" x2="{lx + 16:.1f}" y2="21" stroke="{color}" stroke-width="2.5"{dash_attr}/>'
        )
        out.append(
            f'<text x="{lx + 21:.1f}" y="25" fill="#cbd5e1" font-family="sans-serif" font-size="11">{html.escape(name)}</text>'
        )
        lx -= 12

    # Horizontal grid + Y ticks
    ticks = 5
    for t in range(ticks + 1):
        val = y_max * t / ticks
        y = y_pos(val)
        out.append(
            f'<line x1="{ml}" y1="{y:.1f}" x2="{width - mr}" y2="{y:.1f}" stroke="#1e293b" stroke-width="1" stroke-dasharray="3,3"/>'
        )
        out.append(
            f'<text x="{ml - 8}" y="{y + 4:.1f}" text-anchor="end" fill="#94a3b8" font-family="monospace" font-size="11">{html.escape(fmt_tick(val, y_max, unit))}</text>'
        )

    # Vertical config-change annotations
    for idx, note in annotations or []:
        if 0 <= idx < n:
            ax = x_pos(idx)
            out.append(
                f'<line x1="{ax:.1f}" y1="{mt}" x2="{ax:.1f}" y2="{mt + ph}" stroke="#64748b" stroke-width="1" stroke-dasharray="4,3"/>'
            )
            out.append(
                f'<text x="{ax + 4:.1f}" y="{mt + 12}" fill="#94a3b8" font-family="monospace" font-size="9">{html.escape(note)}</text>'
            )

    # Y axis label
    out.append(
        f'<text x="16" y="{mt + ph / 2:.1f}" fill="#94a3b8" font-family="sans-serif" font-size="11" '
        f'text-anchor="middle" transform="rotate(-90 16 {mt + ph / 2:.1f})">{html.escape(y_label)}</text>'
    )

    # X axis ticks
    step = max(1, (n + 11) // 12)
    for i, lbl in enumerate(labels):
        if i % step != 0 and i != n - 1:
            continue
        x = x_pos(i)
        out.append(
            f'<text x="{x:.1f}" y="{height - mb + 18}" text-anchor="middle" fill="#94a3b8" font-family="monospace" font-size="10">{html.escape(lbl)}</text>'
        )

    # Axes
    out.append(f'<line x1="{ml}" y1="{mt}" x2="{ml}" y2="{mt + ph}" stroke="#475569" stroke-width="1.2"/>')
    out.append(f'<line x1="{ml}" y1="{mt + ph}" x2="{width - mr}" y2="{mt + ph}" stroke="#475569" stroke-width="1.2"/>')

    # Series lines + dots + latest value labels (offset vertically to reduce overlap)
    last_label_ys: list[float] = []
    for _, color, dash, ys in series:
        pts = [(x_pos(i), y_pos(v), v) for i, v in enumerate(ys) if v is not None]
        if len(pts) >= 2:
            d = " ".join(f"{'M' if j == 0 else 'L'}{x:.1f},{y:.1f}" for j, (x, y, _) in enumerate(pts))
            dash_attr = f' stroke-dasharray="{dash}"' if dash else ""
            out.append(f'<path d="{d}" fill="none" stroke="{color}" stroke-width="2.2"{dash_attr}/>')
        for x, y, _ in pts:
            out.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="3.4" fill="{color}"/>')
        if pts:
            lx_p, ly_p, lv = pts[-1]
            lbl_y = max(mt + 12, ly_p - 8)
            for prev_y in last_label_ys:
                if abs(lbl_y - prev_y) < 13:
                    lbl_y = min(mt + ph - 6, prev_y + 14)
            last_label_ys.append(lbl_y)
            anchor = "end" if lx_p > width - mr - 40 else "middle"
            out.append(
                f'<text x="{lx_p:.1f}" y="{lbl_y:.1f}" text-anchor="{anchor}" fill="{color}" font-family="monospace" font-size="10" font-weight="600">{html.escape(fmt_compact(lv, unit))}</text>'
            )

    out.append("</svg>\n")
    return "\n".join(out)


def render_svg_small_multiples(
    title: str,
    subtitle: str,
    labels: list[str],
    panels: list[dict],
    cols: int = 2,
    unit: str = "s",
    annotations: list[tuple[int, str]] | None = None,
    width: int = 900,
    panel_h: int = 215,
) -> str:
    """Render an SVG grid of independent Y-axis panels (Small Multiples).

    Each panel dict has:
      - title: str
      - series: list[(name, color, dasharray, list[float | None])]
      - lower_is_better: bool (default True)
    """
    rows = (len(panels) + cols - 1) // cols
    header_h = 54
    pad = 14
    gap = 14
    height = header_h + rows * panel_h + (rows - 1) * gap + pad
    panel_w = (width - 2 * pad - (cols - 1) * gap) / cols

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" rx="8" fill="#0f172a"/>',
        f'<text x="{pad + 4}" y="25" fill="#e2e8f0" font-family="sans-serif" font-size="14" font-weight="600">{html.escape(title)}</text>',
        f'<text x="{pad + 4}" y="42" fill="#94a3b8" font-family="sans-serif" font-size="11">{html.escape(subtitle)}</text>',
    ]

    # Shared legend from first panel (top right)
    if panels and panels[0].get("series"):
        lx = width - pad - 4
        for name, color, dash, _ in reversed(panels[0]["series"]):
            text_w = len(name) * 6.8 + 24
            lx -= text_w
            dash_attr = f' stroke-dasharray="{dash}"' if dash else ""
            out.append(
                f'<line x1="{lx:.1f}" y1="22" x2="{lx + 14:.1f}" y2="22" stroke="{color}" stroke-width="2.4"{dash_attr}/>'
            )
            out.append(
                f'<text x="{lx + 19:.1f}" y="26" fill="#cbd5e1" font-family="sans-serif" font-size="11">{html.escape(name)}</text>'
            )
            lx -= 10

    n = len(labels)

    for idx_p, panel in enumerate(panels):
        r = idx_p // cols
        c = idx_p % cols
        px = pad + c * (panel_w + gap)
        py = header_h + r * (panel_h + gap)

        out.append(
            f'<rect x="{px:.1f}" y="{py:.1f}" width="{panel_w:.1f}" height="{panel_h}" rx="6" fill="#1e293b" stroke="#334155" stroke-width="1"/>'
        )

        # Latest & delta badge from primary (first) series
        primary_series = panel["series"][0]
        p_vals = [v for v in primary_series[3] if v is not None]
        badge = ""
        badge_color = "#e2e8f0"
        if p_vals:
            cur_v = p_vals[-1]
            badge = fmt_compact(cur_v, unit)
            if len(p_vals) >= 2 and p_vals[-2] > 0:
                d = pct_delta(cur_v, p_vals[-2])
                if abs(d) >= 0.1:
                    badge += f" ({d:+.1f}%)"
                    lower_good = panel.get("lower_is_better", True)
                    is_good = (d < 0) if lower_good else (d > 0)
                    badge_color = "#4ade80" if is_good else "#f87171"

        out.append(
            f'<text x="{px + 12:.1f}" y="{py + 21:.1f}" fill="#f1f5f9" font-family="sans-serif" font-size="12" font-weight="600">{html.escape(panel["title"])}</text>'
        )
        if badge:
            out.append(
                f'<text x="{px + panel_w - 12:.1f}" y="{py + 21:.1f}" text-anchor="end" fill="{badge_color}" font-family="monospace" font-size="11" font-weight="600">{html.escape(badge)}</text>'
            )

        # Plot area inside panel
        ml, mr, mt, mb = 54, 16, 34, 28
        iw = panel_w - ml - mr
        ih = panel_h - mt - mb
        ox = px + ml
        oy = py + mt

        vals = [v for _, _, _, ys in panel["series"] for v in ys if v is not None]
        y_max = max(vals) if vals else 1.0
        if y_max <= 0:
            y_max = 1.0
        y_max *= 1.15

        def x_pos(i: int) -> float:
            if n <= 1:
                return ox + iw / 2.0
            return ox + (i / (n - 1)) * iw

        def y_pos(v: float) -> float:
            return oy + ih - (v / y_max) * ih

        # Y gridlines (4 ticks: 0, 1/3, 2/3, max)
        for t in range(4):
            val = y_max * t / 3
            yy = y_pos(val)
            out.append(
                f'<line x1="{ox:.1f}" y1="{yy:.1f}" x2="{ox + iw:.1f}" y2="{yy:.1f}" stroke="#334155" stroke-width="0.8" stroke-dasharray="2,3"/>'
            )
            out.append(
                f'<text x="{ox - 6:.1f}" y="{yy + 3.5:.1f}" text-anchor="end" fill="#94a3b8" font-family="monospace" font-size="10">{html.escape(fmt_tick(val, y_max, unit))}</text>'
            )

        # Thread-change annotations
        for aidx, anote in annotations or []:
            if 0 <= aidx < n:
                ax = x_pos(aidx)
                out.append(
                    f'<line x1="{ax:.1f}" y1="{oy:.1f}" x2="{ax:.1f}" y2="{oy + ih:.1f}" stroke="#64748b" stroke-width="1" stroke-dasharray="3,3"/>'
                )
                out.append(
                    f'<text x="{ax + 3:.1f}" y="{oy + 10:.1f}" fill="#94a3b8" font-family="monospace" font-size="8.5">{html.escape(anote)}</text>'
                )

        # Axes
        out.append(f'<line x1="{ox:.1f}" y1="{oy:.1f}" x2="{ox:.1f}" y2="{oy + ih:.1f}" stroke="#475569" stroke-width="1"/>')
        out.append(f'<line x1="{ox:.1f}" y1="{oy + ih:.1f}" x2="{ox + iw:.1f}" y2="{oy + ih:.1f}" stroke="#475569" stroke-width="1"/>')

        # X labels (first, middle if >=5, last)
        x_indices = [0] if n == 1 else ([0, n - 1] if n < 5 else [0, n // 2, n - 1])
        for xi in sorted(set(x_indices)):
            xx = x_pos(xi)
            anchor = "start" if xi == 0 and n > 1 else ("end" if xi == n - 1 and n > 1 else "middle")
            out.append(
                f'<text x="{xx:.1f}" y="{oy + ih + 15:.1f}" text-anchor="{anchor}" fill="#94a3b8" font-family="monospace" font-size="9.5">{html.escape(labels[xi])}</text>'
            )

        # Plot series
        for _, color, dash, ys in panel["series"]:
            pts = [(x_pos(i), y_pos(v), v) for i, v in enumerate(ys) if v is not None]
            if len(pts) >= 2:
                d = " ".join(f"{'M' if j == 0 else 'L'}{x:.1f},{y:.1f}" for j, (x, y, _) in enumerate(pts))
                dash_attr = f' stroke-dasharray="{dash}"' if dash else ""
                out.append(f'<path d="{d}" fill="none" stroke="{color}" stroke-width="2.1"{dash_attr}/>')
            for x, y, _ in pts:
                out.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="3.0" fill="{color}"/>')

    out.append("</svg>\n")
    return "\n".join(out)


def find_problem_metric(suite_data: dict | None, pname: str, bk: str, field: str) -> float | None:
    if not suite_data:
        return None
    for r in suite_data.get("results", []):
        if r["name"] == pname and bk in r and r[bk] is not None:
            return r[bk].get(field)
    return None


def thread_change_annotations(entries: list[dict], suite_key: str) -> list[tuple[int, str]]:
    ann: list[tuple[int, str]] = []
    prev_t = None
    for i, e in enumerate(entries):
        sd = e.get(suite_key)
        if not sd:
            continue
        t = sd.get("threads")
        if t is not None and prev_t is not None and t != prev_t:
            ann.append((i, f"{t}T"))
        if t is not None:
            prev_t = t
    return ann


def render_dashboard_md(history: dict, out_dir: str) -> None:
    entries: list[dict] = history.get("entries", [])
    charts_dir = os.path.join(out_dir, "charts")
    os.makedirs(charts_dir, exist_ok=True)

    labels = [e["commit_short"] for e in entries]
    long_ann = thread_change_annotations(entries, "long_mt")

    # 1. 1T Deterministic Total Solve Time SVG
    svg_std_time = render_svg_line_chart(
        "1T Deterministic — Total Solve Time (11 problems, lower is better)",
        "Seconds",
        labels,
        [
            ("FmrsPos (1T)", "#fb923c", "", [e.get("std_1t", {}).get("total_fmrs_secs") for e in entries]),
            ("ShtsumePos (1T)", "#38bdf8", "6,3", [e.get("std_1t", {}).get("total_shtsume_secs") for e in entries]),
        ],
        unit="s",
    )
    with open(os.path.join(charts_dir, "std_1t_time.svg"), "w", encoding="utf-8") as f:
        f.write(svg_std_time)

    # 2. 1T Deterministic Total Nodes SVG (0% runner jitter)
    svg_std_nodes = render_svg_line_chart(
        "1T Deterministic — Total Search Nodes (11 problems, lower is better, 0% CI noise)",
        "Nodes",
        labels,
        [
            ("FmrsPos (1T)", "#fb923c", "", [e.get("std_1t", {}).get("total_fmrs_nodes") for e in entries]),
            ("ShtsumePos (1T)", "#38bdf8", "6,3", [e.get("std_1t", {}).get("total_shtsume_nodes") for e in entries]),
        ],
        unit="nodes",
    )
    with open(os.path.join(charts_dir, "std_1t_nodes.svg"), "w", encoding="utf-8") as f:
        f.write(svg_std_nodes)

    # 3. 1T Deterministic Aggregate kNPS SVG
    svg_std_knps = render_svg_line_chart(
        "1T Deterministic — Aggregate Throughput (kNPS, higher is better)",
        "kNPS",
        labels,
        [
            ("FmrsPos (1T)", "#fb923c", "", [e.get("std_1t", {}).get("agg_fmrs_knps") for e in entries]),
            ("ShtsumePos (1T)", "#38bdf8", "6,3", [e.get("std_1t", {}).get("agg_shtsume_knps") for e in entries]),
        ],
        unit="kNPS",
    )
    with open(os.path.join(charts_dir, "std_1t_knps.svg"), "w", encoding="utf-8") as f:
        f.write(svg_std_knps)

    # 4. Long-Mate Parallel Suite — Per-Problem Time Small Multiples (2x2 independent Y-axes)
    long_defs = [
        ("zuko_01_69", "図巧1番 (69手)"),
        ("zuko_99_kemuri_117", "煙詰 (117手)"),
        ("zuko_100_kotobuki_611", "寿 (611手)"),
        ("microcosmos_1525", "ミクロコスモス (1525手)"),
    ]
    latest = entries[-1]
    latest_long_t = latest.get("long_mt", {}).get("threads", "?")

    long_time_panels = [
        {
            "title": title,
            "lower_is_better": True,
            "series": [
                (
                    f"FmrsPos ({latest_long_t}T)",
                    "#fb923c",
                    "",
                    [find_problem_metric(e.get("long_mt"), pname, "fmrs", "elapsed_secs") for e in entries],
                ),
            ],
        }
        for pname, title in long_defs
    ]
    svg_long_time = render_svg_small_multiples(
        "Long-Mate Parallel Suite — Solve Time per Problem (s, lower is better)",
        "Independent Y-axis per problem (FmrsPos, parallel runner vCPUs, hash=1024MB)",
        labels,
        long_time_panels,
        cols=2,
        unit="s",
        annotations=long_ann,
    )
    with open(os.path.join(charts_dir, "long_mt_time_grid.svg"), "w", encoding="utf-8") as f:
        f.write(svg_long_time)

    # 5. Long-Mate Parallel Suite — Per-Problem Nodes Small Multiples (2x2 independent Y-axes)
    long_nodes_panels = [
        {
            "title": title,
            "lower_is_better": True,
            "series": [
                (
                    f"FmrsPos ({latest_long_t}T)",
                    "#a78bfa",
                    "",
                    [find_problem_metric(e.get("long_mt"), pname, "fmrs", "nodes") for e in entries],
                ),
            ],
        }
        for pname, title in long_defs
    ]
    svg_long_nodes = render_svg_small_multiples(
        "Long-Mate Parallel Suite — Search Nodes per Problem (lower is better)",
        "Independent Y-axis per problem (separates algorithmic node reduction from kNPS throughput)",
        labels,
        long_nodes_panels,
        cols=2,
        unit="nodes",
        annotations=long_ann,
    )
    with open(os.path.join(charts_dir, "long_mt_nodes_grid.svg"), "w", encoding="utf-8") as f:
        f.write(svg_long_nodes)

    # 6. 1T Deterministic Long Problems — Per-Problem Time Small Multiples (1x3 independent Y-axes)
    std_long_defs = [
        ("zuko_01_69", "図巧1番 (69手)"),
        ("zuko_99_kemuri_117", "煙詰 (117手)"),
        ("zuko_100_kotobuki_611", "寿 (611手)"),
    ]
    std_long_panels = [
        {
            "title": title,
            "lower_is_better": True,
            "series": [
                (
                    "FmrsPos (1T)",
                    "#fb923c",
                    "",
                    [find_problem_metric(e.get("std_1t"), pname, "fmrs", "elapsed_secs") for e in entries],
                ),
                (
                    "ShtsumePos (1T)",
                    "#38bdf8",
                    "6,3",
                    [find_problem_metric(e.get("std_1t"), pname, "shtsume", "elapsed_secs") for e in entries],
                ),
            ],
        }
        for pname, title in std_long_defs
    ]
    svg_std_long = render_svg_small_multiples(
        "1T Deterministic Long Problems — Solve Time per Problem (s, lower is better)",
        "Independent Y-axis per problem (FmrsPos solid vs ShtsumePos dashed, 1T hash=256MB)",
        labels,
        std_long_panels,
        cols=3,
        unit="s",
        width=900,
        panel_h=205,
    )
    with open(os.path.join(charts_dir, "std_1t_long_grid.svg"), "w", encoding="utf-8") as f:
        f.write(svg_std_long)

    # Render README.md
    prev = entries[-2] if len(entries) >= 2 else None
    std = latest.get("std_1t") or {}
    lmt = latest.get("long_mt") or {}
    prev_std = prev.get("std_1t") if prev else None
    prev_lmt = prev.get("long_mt") if prev else None

    sh_t = std.get("total_shtsume_secs", 0.0)
    fm_t = std.get("total_fmrs_secs", 0.0)
    ratio = f"{sh_t / fm_t:.2f}x" if fm_t > 0 else "-"

    lines = [
        "# fmrs::futsuu — Benchmark History",
        "",
        f"- **Latest Commit**: [`{latest['commit_short']}`]({latest['commit_url']}) ({latest['timestamp']})",
        f"- **Recorded Runs**: {len(entries)}",
        f"- **Struct Sizes**: `ShtsumePos` = {latest.get('sizeof_shtsume_pos', '-')} B, `FmrsPos` = {latest.get('sizeof_fmrs_pos', '-')} B",
    ]
    if std:
        lines.extend([
            f"- **1T Deterministic (`std`, 11 problems, hash={std.get('hash_mb', 256)}MB)**: "
            f"`FmrsPos` **{fm_t:.3f}s** ({std.get('total_fmrs_nodes', 0):,} nodes, {std.get('agg_fmrs_knps', 0.0):.1f} kNPS) vs "
            f"`ShtsumePos` {sh_t:.3f}s ({std.get('total_shtsume_nodes', 0):,} nodes, {std.get('agg_shtsume_knps', 0.0):.1f} kNPS) — "
            f"`SH / FMRS` = **{ratio}**",
        ])
    if lmt:
        lines.extend([
            f"- **Long-Mate Parallel (`long`, {len(lmt.get('results', []))} problems, {lmt.get('threads', '?')}T, hash={lmt.get('hash_mb', 1024)}MB)**: "
            f"`FmrsPos` **{lmt.get('total_fmrs_secs', 0.0):.3f}s** ({lmt.get('total_fmrs_nodes', 0):,} nodes, {lmt.get('agg_fmrs_knps', 0.0):.1f} kNPS)",
        ])

    lines.extend([
        "",
        "## Long-Mate Parallel Trends (Independent Y-Axis per Problem)",
        "",
        "![Long-Mate Parallel Solve Time](./charts/long_mt_time_grid.svg)",
        "",
        "![Long-Mate Parallel Search Nodes](./charts/long_mt_nodes_grid.svg)",
        "",
        "## 1T Deterministic Trends (0% Thread-Scheduling Noise)",
        "",
        "![1T Total Search Nodes](./charts/std_1t_nodes.svg)",
        "",
        "![1T Total Solve Time](./charts/std_1t_time.svg)",
        "",
        "![1T Aggregate Throughput](./charts/std_1t_knps.svg)",
        "",
        "![1T Long Problems per Problem](./charts/std_1t_long_grid.svg)",
        "",
    ])

    if lmt:
        prev_lmap = {r["name"]: r for r in (prev_lmt.get("results", []) if prev_lmt else [])}
        lines.extend([
            f"## Latest Long-Mate Parallel Suite (`{latest['commit_short']}`, {lmt.get('threads', '?')}T, hash={lmt.get('hash_mb', 1024)}MB)",
            "",
            "| Problem | Note | Result | Nodes | Time | kNPS | GC |",
            "| --- | --- | --- | ---: | ---: | ---: | ---: |",
        ])
        for r in lmt.get("results", []):
            fm = r.get("fmrs")
            if not fm:
                continue
            pr = prev_lmap.get(r["name"], {}).get("fmrs")
            dt = f" ({pct_delta(fm['elapsed_secs'], pr['elapsed_secs']):+.1f}%)" if pr else ""
            dn = f" ({pct_delta(fm['nodes'], pr['nodes']):+.1f}%)" if pr and pr.get("nodes") != fm["nodes"] else ""
            lines.append(
                f"| `{r['name']}` | {r.get('note', '')} | {fmt_mate(fm)} | "
                f"{fm['nodes']:,}{dn} | {fm['elapsed_secs']:.3f}s{dt} | {fm['knps']:.1f} | {fm.get('gc_num', 0)} |"
            )
        lines.append("")

    if std:
        prev_smap = {r["name"]: r for r in (prev_std.get("results", []) if prev_std else [])}
        lines.extend([
            f"## Latest 1T Deterministic Suite (`{latest['commit_short']}`, 1T, hash={std.get('hash_mb', 256)}MB)",
            "",
            "| Problem | Note | SH Res | SH Nodes | SH Time | SH kNPS | FMRS Res | FMRS Nodes | FMRS Time | FMRS kNPS | SH/FMRS |",
            "| --- | --- | --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | ---: |",
        ])
        for r in std.get("results", []):
            sh = r.get("shtsume")
            fm = r.get("fmrs")
            pr = prev_smap.get(r["name"])
            sh_dt = (
                f" ({pct_delta(sh['elapsed_secs'], pr['shtsume']['elapsed_secs']):+.1f}%)"
                if sh and pr and "shtsume" in pr
                else ""
            )
            fm_dt = (
                f" ({pct_delta(fm['elapsed_secs'], pr['fmrs']['elapsed_secs']):+.1f}%)"
                if fm and pr and "fmrs" in pr
                else ""
            )
            sp = f"{sh['elapsed_secs'] / fm['elapsed_secs']:.2f}x" if sh and fm and fm["elapsed_secs"] > 0 else "-"
            sh_nodes = f"{sh['nodes']:,}" if sh else "-"
            sh_time = f"{sh['elapsed_secs']:.3f}s{sh_dt}" if sh else "-"
            sh_knps = f"{sh['knps']:.1f}" if sh else "-"
            fm_nodes = f"{fm['nodes']:,}" if fm else "-"
            fm_time = f"{fm['elapsed_secs']:.3f}s{fm_dt}" if fm else "-"
            fm_knps = f"{fm['knps']:.1f}" if fm else "-"
            lines.append(
                f"| `{r['name']}` | {r.get('note', '')} | {fmt_mate(sh)} | "
                f"{sh_nodes} | {sh_time} | {sh_knps} | "
                f"{fmt_mate(fm)} | {fm_nodes} | {fm_time} | {fm_knps} | {sp} |"
            )
        lines.append("")

    lines.extend([
        "## Recorded Runs",
        "",
        "| Commit | Date (UTC) | 1T FMRS Nodes | 1T FMRS Time | 1T SH/FMRS | Long MT Time (T) | Microcosmos | Reasons / Note |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |",
    ])
    for e in reversed(entries):
        es = e.get("std_1t") or {}
        el = e.get("long_mt") or {}
        sp = (
            f"{es['total_shtsume_secs'] / es['total_fmrs_secs']:.2f}x"
            if es.get("total_fmrs_secs", 0) > 0 and es.get("total_shtsume_secs", 0) > 0
            else "-"
        )
        mc = find_problem_metric(el, "microcosmos_1525", "fmrs", "elapsed_secs")
        mc_str = f"{mc:.2f}s" if mc is not None else "-"
        l_str = f"{el['total_fmrs_secs']:.2f}s ({el.get('threads', '?')}T)" if el else "-"
        s_nodes = f"{es['total_fmrs_nodes']:,}" if es else "-"
        s_time = f"{es['total_fmrs_secs']:.3f}s" if es else "-"
        note = e.get("note") or e.get("message") or ""
        reasons = ", ".join(e.get("reasons", []))
        desc = f"{note} (`{reasons}`)" if reasons else note
        lines.append(
            f"| [`{e['commit_short']}`]({e['commit_url']}) | {e['timestamp'].replace('T', ' ').replace('Z', '')} | "
            f"{s_nodes} | {s_time} | {sp} | {l_str} | {mc_str} | {desc} |"
        )

    with open(os.path.join(out_dir, "README.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


def format_summary_md(entry: dict, prev: dict | None, recorded: bool, reasons: list[str]) -> str:
    lines = [
        f"### fmrs::futsuu Benchmark Summary (`{entry['commit_short']}`)",
        f"- **Status**: {'Recorded to history' if recorded else 'Skipped (no significant change)'} ({', '.join(reasons) if reasons else 'within noise threshold'})",
    ]
    if entry.get("note"):
        lines.append(f"- **Note**: {entry['note']}")
    lines.append(
        f"- **Struct sizes**: `ShtsumePos` = {entry.get('sizeof_shtsume_pos', '-')} B, "
        f"`FmrsPos` = {entry.get('sizeof_fmrs_pos', '-')} B"
    )

    std = entry.get("std_1t")
    prev_std = prev.get("std_1t") if prev else None
    if std:
        sh_t = std["total_shtsume_secs"]
        fm_t = std["total_fmrs_secs"]
        ratio = f"{sh_t / fm_t:.2f}x" if fm_t > 0 else "-"
        lines.extend([
            "",
            "#### 1T Deterministic Suite (11 problems, hash=256MB)",
            "| Metric | `ShtsumePos` | `FmrsPos` | `SH / FMRS` |",
            "| --- | ---: | ---: | ---: |",
            f"| Total Nodes | {std['total_shtsume_nodes']:,}"
            + (f" ({pct_delta(std['total_shtsume_nodes'], prev_std['total_shtsume_nodes']):+.2f}%)" if prev_std else "")
            + f" | {std['total_fmrs_nodes']:,}"
            + (f" ({pct_delta(std['total_fmrs_nodes'], prev_std['total_fmrs_nodes']):+.2f}%)" if prev_std else "")
            + " | - |",
            f"| Total Time | {sh_t:.3f}s"
            + (f" ({pct_delta(sh_t, prev_std['total_shtsume_secs']):+.1f}%)" if prev_std else "")
            + f" | {fm_t:.3f}s"
            + (f" ({pct_delta(fm_t, prev_std['total_fmrs_secs']):+.1f}%)" if prev_std else "")
            + f" | {ratio} |",
            f"| Aggregate kNPS | {std['agg_shtsume_knps']:.1f}"
            + (f" ({pct_delta(std['agg_shtsume_knps'], prev_std['agg_shtsume_knps']):+.1f}%)" if prev_std else "")
            + f" | {std['agg_fmrs_knps']:.1f}"
            + (f" ({pct_delta(std['agg_fmrs_knps'], prev_std['agg_fmrs_knps']):+.1f}%)" if prev_std else "")
            + " | - |",
        ])

    lmt = entry.get("long_mt")
    prev_lmt = prev.get("long_mt") if prev else None
    if lmt:
        prev_lmap = {r["name"]: r for r in (prev_lmt.get("results", []) if prev_lmt else [])}
        lines.extend([
            "",
            f"#### Long-Mate Parallel Suite (`FmrsPos`, {lmt.get('threads', '?')}T, hash={lmt.get('hash_mb', 1024)}MB)",
            "| Problem | Result | Nodes | Time | kNPS |",
            "| --- | --- | ---: | ---: | ---: |",
        ])
        for r in lmt.get("results", []):
            fm = r.get("fmrs")
            if not fm:
                continue
            pr = prev_lmap.get(r["name"], {}).get("fmrs")
            dt = f" ({pct_delta(fm['elapsed_secs'], pr['elapsed_secs']):+.1f}%)" if pr else ""
            lines.append(
                f"| `{r['name']}` | {fmt_mate(fm)} | {fm['nodes']:,} | {fm['elapsed_secs']:.3f}s{dt} | {fm['knps']:.1f} |"
            )

    return "\n".join(lines) + "\n"


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--std-input", required=True, help="Path to 1T std futsuu_bench --json output")
    p.add_argument("--long-input", default="", help="Optional path to MT long futsuu_bench --json output")
    p.add_argument("--history", required=True, help="Path to history.json")
    p.add_argument("--render-dir", default="", help="Directory to render README.md and charts/*.svg")
    p.add_argument("--commit", default=os.environ.get("GITHUB_SHA", "local"))
    p.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "ogiekako/fmrs"))
    p.add_argument("--message", default="")
    p.add_argument("--note", default="")
    p.add_argument("--event", default=os.environ.get("GITHUB_EVENT_NAME", "local"))
    p.add_argument("--threshold-pct", type=float, default=3.0)
    p.add_argument("--long-threshold-pct", type=float, default=5.0)
    p.add_argument("--only-if-significant", action="store_true")
    p.add_argument("--force", action="store_true")
    p.add_argument("--summary-md", default="", help="Optional path to write Markdown summary")
    p.add_argument("--github-output", default="", help="Optional path to write GITHUB_OUTPUT key=value")
    args = p.parse_args()

    with open(args.std_input, "r", encoding="utf-8") as f:
        std_data = json.load(f)

    long_data = None
    if args.long_input and os.path.exists(args.long_input):
        with open(args.long_input, "r", encoding="utf-8") as f:
            long_data = json.load(f)

    if os.path.exists(args.history):
        with open(args.history, "r", encoding="utf-8") as f:
            history = json.load(f)
    else:
        history = {"schema_version": 2, "repo": args.repo, "entries": []}

    entries: list[dict] = history.get("entries", [])
    prev_entry = entries[-1] if entries else None
    if prev_entry and prev_entry.get("commit") == args.commit:
        prev_entry = entries[-2] if len(entries) >= 2 else None

    now_iso = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    commit_short = args.commit[:7]
    entry: dict = {
        "timestamp": now_iso,
        "commit": args.commit,
        "commit_short": commit_short,
        "commit_url": f"https://github.com/{args.repo}/commit/{args.commit}",
        "message": args.message.splitlines()[0] if args.message else "",
        "note": args.note,
        "event": args.event,
        "sizeof_shtsume_pos": std_data.get("sizeof_shtsume_pos"),
        "sizeof_fmrs_pos": std_data.get("sizeof_fmrs_pos"),
        "std_1t": std_data,
    }
    if long_data is not None:
        entry["long_mt"] = long_data

    significant, reasons = evaluate_significance(
        entry, prev_entry, args.threshold_pct, args.long_threshold_pct
    )
    entry["significant"] = significant
    entry["reasons"] = reasons

    should_record = args.force or (not args.only_if_significant) or significant
    if should_record:
        replaced = False
        for i, e in enumerate(entries):
            if e.get("commit") == args.commit:
                entries[i] = entry
                replaced = True
                break
        if not replaced:
            entries.append(entry)
        history["schema_version"] = 2
        history["repo"] = args.repo
        history["entries"] = entries
        os.makedirs(os.path.dirname(os.path.abspath(args.history)), exist_ok=True)
        with open(args.history, "w", encoding="utf-8") as f:
            json.dump(history, f, ensure_ascii=False, indent=2)

    if args.render_dir and history.get("entries"):
        render_dashboard_md(history, args.render_dir)

    md = format_summary_md(entry, prev_entry, should_record, reasons)
    if args.summary_md:
        with open(args.summary_md, "a", encoding="utf-8") as f:
            f.write(md)
    else:
        sys.stdout.write(md)

    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as f:
            f.write(f"recorded={'true' if should_record else 'false'}\n")
            f.write(f"significant={'true' if significant else 'false'}\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
