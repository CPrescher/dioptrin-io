"""Summarize matched small codec benchmarks, retaining noisy/negative cases."""
import json
import os
from pathlib import Path
import statistics
import sys


def summarize(record):
    assert record["validation"] and all(v["all_counts_match"] for v in record["validation"])
    rows = []
    comparisons = []
    for source in record["sources"]:
        case = source["case"]
        for workers in record["worker_counts"]:
            samples = [s for s in record["samples"] if s["case"] == case and s["workers"] == workers]
            kernels = sorted({s["unshuffle"] for s in samples})
            baseline = {s["repeat"]: s for s in samples
                        if s["unshuffle"] == source["automatic_unshuffle"] and s["lz4"] == "Rust"}
            assert set(baseline) == set(range(record["repeats"]))
            for kernel in kernels:
                rust = {s["repeat"]: s for s in samples if s["unshuffle"] == kernel and s["lz4"] == "Rust"}
                native = {s["repeat"]: s for s in samples if s["unshuffle"] == kernel and s["lz4"] == "Native"}
                assert set(rust) == set(native) == set(baseline)
                for lz4, runs in [("Rust", rust), ("Native", native)]:
                    assert any(v["case"] == case and v["workers"] == workers and
                               v["unshuffle"] == kernel and v["lz4"] == lz4 and
                               v["frames"] == source["shape"][0] and v["all_counts_match"]
                               for v in record["validation"])
                    assert all(s["frames"] == baseline[i]["frames"] and
                               s["seconds"] > 0 and s["fps"] > 0
                               for i, s in runs.items())
                ratios = [rust[i]["seconds"] / native[i]["seconds"] for i in sorted(rust)]
                comparisons.append({"case": case, "dtype": source["dtype"], "shape": source["shape"],
                                    "workers": workers, "unshuffle": kernel,
                                    "automatic_unshuffle": source["automatic_unshuffle"],
                                    "native_over_rust_median": statistics.median(ratios),
                                    "native_over_rust_min": min(ratios), "native_over_rust_max": max(ratios)})
                for lz4, runs in [("Rust", rust), ("Native", native)]:
                    fps = [s["fps"] for s in runs.values()]
                    speed = [baseline[i]["seconds"] / runs[i]["seconds"] for i in sorted(runs)]
                    rows.append({"case": case, "workers": workers, "unshuffle": kernel, "lz4": lz4,
                                 "median_fps": statistics.median(fps), "min_fps": min(fps), "max_fps": max(fps),
                                 "median_mib_s": statistics.median(s["decoded_mib_s"] for s in runs.values()),
                                 "paired_over_auto_rust": statistics.median(speed)})
    return {"arch": record["arch"], "os": record["os"], "cpu": record["cpu"],
            "allocated_cpus": record["allocated_cpus"], "commit": record["commit"],
            "sources": record["sources"], "validation_passes": len(record["validation"]),
            "all_counts_match": True, "lz4_comparisons": comparisons, "configurations": rows}


def markdown(summary):
    lines = ["## Small codec qualification", "",
             f'CPU: {summary["cpu"]}; {summary["os"]}/{summary["arch"]}; allocated CPUs: {summary["allocated_cpus"]}.',
             "", "Known generated original counts; uint16/uint32, extrema, dense/sparse and short frames/tails. "
             "Warm file reads + source checks + decode + typed pixel delivery; no decoded cache. "
             "Alternating matched pairs; sample counts and duration are recorded in the raw JSON. Shared-runner timings are diagnostic, with no speed threshold gating CI.",
             "", "### Native versus Rust LZ4 with the automatically selected unshuffle kernel", "",
             "| Case | Workers | Unshuffle | Rust fps | Native fps | Paired native/Rust (range) |",
             "| --- | ---: | --- | ---: | ---: | ---: |"]
    for row in summary["lz4_comparisons"]:
        if row["unshuffle"] != row["automatic_unshuffle"]:
            continue
        matches = [r for r in summary["configurations"] if
                   (r["case"], r["workers"], r["unshuffle"]) == (row["case"], row["workers"], row["unshuffle"])]
        rates = {r["lz4"]: r["median_fps"] for r in matches}
        lines.append(f'| {row["case"]} | {row["workers"]} | {row["unshuffle"]} | {rates["Rust"]:.1f} | '
                     f'{rates["Native"]:.1f} | {row["native_over_rust_median"]:.3f}× '
                     f'({row["native_over_rust_min"]:.3f}–{row["native_over_rust_max"]:.3f}) |')
    lines += ["", "### Fastest observed configuration per workload", "",
              "Rankings use median paired speedups against Auto + Rust among the tested kernels. Small differences can be scheduling noise; "
              "this table does not automatically change platform defaults.", "",
              "| Case | Workers | Observed configuration | Median MiB/s | Paired speedup vs Auto + Rust |",
              "| --- | ---: | --- | ---: | ---: |"]
    for source in summary["sources"]:
        for workers in sorted({r["workers"] for r in summary["configurations"]}):
            rows = [r for r in summary["configurations"] if r["case"] == source["case"] and r["workers"] == workers]
            best = max(rows, key=lambda r: r["paired_over_auto_rust"])
            lines.append(f'| {source["case"]} | {workers} | {best["unshuffle"]} + {best["lz4"]} | '
                         f'{best["median_mib_s"]:.1f} | {best["paired_over_auto_rust"]:.3f}× |')
    lines += ["", f'All {summary["validation_passes"]} original-count validation passes matched. '
              "Raw per-pass timings, exact shapes, generated-count hashes and source/compiler metadata are retained in the artifacts."]
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    result = summarize(json.loads(Path(sys.argv[1]).read_text()))
    if len(sys.argv) > 2:
        Path(sys.argv[2]).write_text(json.dumps(result, indent=2) + "\n")
    text = markdown(result)
    print(text)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(path).open("a", encoding="utf-8") as output:
            output.write(text)
