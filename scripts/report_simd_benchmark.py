"""Render a retained benchmark JSON as a GitHub job summary or stdout."""
import json
import os
from pathlib import Path
import sys

record = json.loads(Path(sys.argv[1]).read_text())
lines = ['## Native scalar/SIMD read + decode', '',
         '**Deterministic synthetic 9M-sized uint32 frames; not original beamline data.**', '',
         f'CPU allocation: {record["logical_cpus"]}; automatic backend: {record["automatic_backend"]}.', '',
         '| Input | Workers | Backend | Median fps | Range fps | Gain over scalar |',
         '|---|---:|---|---:|---:|---:|']
for row in record['summary']:
    scalar = next(s for s in record['summary'] if s['case'] == row['case']
                  and s['workers'] == row['workers'] and s['backend'] == 'Scalar')
    gain = 100 * (row['median_fps'] / scalar['median_fps'] - 1)
    lines.append(f'| {row["case"]} | {row["workers"]} | {row["backend"]} | '
                 f'{row["median_fps"]:.2f} | {row["min_fps"]:.2f}–{row["max_fps"]:.2f} | {gain:+.1f}% |')
assert all(v['all_counts_match'] for v in record['validation'])
lines += ['', f'All {len(record["validation"])} whole-file count-validation passes match.', '', record['boundaries']]
text = '\n'.join(lines) + '\n'
print(text)
if path := os.environ.get('GITHUB_STEP_SUMMARY'):
    with Path(path).open('a', encoding='utf-8') as summary:
        summary.write(text)
