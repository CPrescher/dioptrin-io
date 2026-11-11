"""Summarize complete paired codec runs; never select only the fastest pass."""
import json
import math
from pathlib import Path
import statistics
import sys


def summarize(report):
    variants = [v["name"] for v in report["variants"]]
    assert len(report["validation"]) == len(variants), "incomplete count validation"
    assert all(v["all_independent_counts_match"] for v in report["validation"])
    groups = sorted({(r["mode"], r["poisson"]) for r in report["timings"]})
    rows = []
    for mode, errors in groups:
        controls = {r["pass"]: r for r in report["timings"]
                    if r["variant"] == "main" and (r["mode"], r["poisson"]) == (mode, errors)}
        assert len(controls) == report["passes"], "incomplete baseline passes"
        for variant in variants:
            runs = [r for r in report["timings"] if r["variant"] == variant
                    and (r["mode"], r["poisson"]) == (mode, errors)]
            assert len(runs) == report["passes"], "incomplete variant passes"
            rates = [r["fps"] for r in runs]
            ratios = [controls[r["pass"]]["seconds"] / r["seconds"] for r in runs]
            if mode != "decode":
                assert len({r["output_sha256"] for r in runs} |
                           {r["output_sha256"] for r in controls.values()}) == 1
            rows.append({"mode": mode, "poisson": errors, "variant": variant,
                         "median_fps": statistics.median(rates), "min_fps": min(rates),
                         "max_fps": max(rates), "paired_median_speedup": statistics.median(ratios),
                         "paired_min_speedup": min(ratios), "paired_max_speedup": max(ratios)})
    latency = []
    for errors in [False, True]:
        controls = {(r["pass"], r["frame"]): r for r in report["single_frame"]
                    if r["variant"] == "main" and r["poisson"] == errors}
        assert len(controls) == report["passes"] * 24, "incomplete single-frame passes"
        for variant in variants:
            runs = [r for r in report["single_frame"] if r["variant"] == variant and r["poisson"] == errors]
            assert len(runs) == len(controls)
            times = sorted(r["seconds"] * 1000 for r in runs)
            ratios = []
            for run in runs:
                control = controls[(run["pass"], run["frame"])]
                assert run["output_sha256"] == control["output_sha256"]
                ratios.append(control["seconds"] / run["seconds"])
            latency.append({"poisson": errors, "variant": variant,
                            "samples": len(times), "median_ms": statistics.median(times),
                            "p95_ms": times[math.ceil(0.95 * len(times)) - 1],
                            "paired_median_speedup": statistics.median(ratios)})
    return {"source_sha256": report["source_sha256"], "validation": report["validation"],
            "whole_file": rows, "single_frame": latency}


if __name__ == "__main__":
    result = summarize(json.loads(Path(sys.argv[1]).read_text()))
    if len(sys.argv) > 2:
        Path(sys.argv[2]).write_text(json.dumps(result, indent=2) + "\n")
    for row in result["whole_file"]:
        print(f'{row["mode"]:12} Poisson={str(row["poisson"]):5} {row["variant"]:15} '
              f'{row["median_fps"]:7.2f} fps; paired {row["paired_median_speedup"]:.3f}x '
              f'({row["paired_min_speedup"]:.3f}–{row["paired_max_speedup"]:.3f})')
    for row in result["single_frame"]:
        print(f'single-frame Poisson={str(row["poisson"]):5} {row["variant"]:15} '
              f'{row["median_ms"]:.3f}/{row["p95_ms"]:.3f} ms median/p95; '
              f'paired {row["paired_median_speedup"]:.3f}x')
