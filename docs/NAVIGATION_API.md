# Frame navigation

The optional `wrappers` feature exposes `navigation::FrameSource` and
`navigation::Cursor<S>`. `wrapped::Reader` implements the trait. A file-series
reader can implement the same trait after it has indexed its explicit file list.
Navigation does not infer neighboring filenames, discover a new codec, combine
readouts, normalize frame shapes or convert sample types.

```rust,ignore
let mut cursor = dioptrin_io::navigation::Cursor::new(reader);
cursor.next()?; // BeforeFirst -> frame 0
let original_samples = cursor.buffer().unwrap().pixels().unwrap();
cursor.read_at(4)?;
cursor.previous()?; // frame 3
cursor.seek_relative(-2)?; // frame 1
```

The cursor owns its source and one reusable `wrapped::FrameBuffer`. A successful
move returns a borrow of that buffer. The borrow ends before the next mutable
operation, so a later read cannot silently mutate samples that remain borrowed.
`source()` exposes backend metadata; `source_mut()` clears visible output before
returning mutable access. `into_inner()` returns ownership of the source.

The frame count is frozen at construction and checked before and after reads.
Each source retains responsibility for checking its complete source graph on
every read. Source changes require reopening; there is no live refresh or
decoded frame cache. Output uses the selected backend's existing limits. One
frame and the backend's bounded reusable scratch are retained, independently of
the total frame count.

| Operation | Behavior |
| --- | --- |
| Construction | `Position::BeforeFirst`, no visible frame |
| `read_at(i)` | Read zero-based index `i`; index >= count selects `AfterLast` and errors |
| `read_at_signed(i)` | Signed absolute index; negatives select `BeforeFirst` and error, unrepresentable positive indices select `AfterLast` |
| `next()` | Move +1; `BeforeFirst` moves to frame 0 |
| `previous()` | Move -1; `AfterLast` moves to the final frame |
| `first()` / `last()` | Read index 0 / count - 1 |
| `seek_relative(delta)` | Checked signed offset; the boundaries act as indices -1 and count |
| Relative offset 0 | Reread the anchored frame; either boundary errors |
| Overshoot / arithmetic overflow | Select the corresponding boundary and error; never wrap |
| Backend failure | Retain the preceding logical anchor and return its error |
| Empty source | No decoder read; `first` selects `AfterLast`, `last` selects `BeforeFirst` |

Every failed operation clears `buffer()` and `current_index()`, including an
implementation that writes an output buffer and then returns an error.
`position()` describes the navigation anchor, not proof that its samples remain
visible. After a backend error, retrying `next()` retries the same requested next
frame from the unchanged anchor. Boundary failures deliberately change the
anchor so reversing direction reenters the sequence at the corresponding end.

The cursor's controls are a native API, not an implementation of Python
`Iterator`. Each successful operation is an explicit read with source validation;
repeated end-of-sequence calls report the same boundary without wrapping.
The signed absolute helper rejects negative indices rather than wrapping to the
end of the source. Negative relative offsets are admitted and checked, including
`i64::MIN` without negation overflow.

## Reference qualification and validation

Pinned FabIO 2026.6.0 commit `9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`
provides in-range stack/file-series navigation reference behavior. Its
[`FileSeries._get_frame`](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/file_series.py#L780)
explicitly rejects negative frame numbers. The native API follows that
nonwrapping boundary contract. Filename traversal remains a separate explicit
series operation rather than the base `FabioImage.getframe` filename heuristic.

Two legacy EDF qualifications prevent a blanket control-parity claim:
[`EdfImage.getframe`](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/edfimage.py#L1336)
checks `num < nframes` without checking `num >= 0`, so negative Python list
indexing can select the final frame. Its
[`test_previous_multi`](https://github.com/silx-kit/fabio/blob/9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece/src/fabio/test/codecs/test_edfimage.py#L344)
labels the expected result frame 0 but compares it with frame 1. This weak
assertion is not evidence that repeating frame 1 is correct. Native `previous`
selects exactly index - 1, verified against independent distinguishable values.

`tests/navigation.rs` generates small independent arrays directly: a concatenated
EDF stack with big-endian U16 extrema, little-endian I32 extrema and big-endian
F64 negative zero/infinity, with a different shape per frame. Tests verify all
controls, reverse/random access, exact native values, both boundary states,
empty sources, extreme offsets, frame-count changes before/after decoding,
post-write failures, and path replacement by identical bytes. No detector corpus,
Python runtime or new codec implementation is needed for these tests.

## Pinned file-series and filename audit

The [2026-11-06 API report](series/2026-11-06-series-navigation.json) runs the
unchanged pinned `test_file_series.py`, `test_filename_steps.py` and
`test_filenames.py`: **37 tests, zero failures, errors or skips**. It records
actual returned frames and failed requests, not a replacement Python API.

Nineteen observed `FileSeries` instances provide 49 actual requests, comprising
40 successful reads (including 20 iterator yields) and nine expected errors.
The filename-object tests provide another 25 actual image opens. Fresh separate
reference readers establish file/frame boundaries, total counts and values for
independent navigation traces. The resulting 515 native actions comprise 290
successful reads and 225 error returns. Each success compares dtype, shape,
canonical little-endian sample SHA256, global index, file ordinal, local frame,
source path and header fields. All 3,429 reference header-field occurrences
match after the documented EDF key-case presentation. Error actions must expose
neither a current index nor pixels or a frame record.

The 36 observed filename helper calls comprise 34 exact filename matches and
two qualified nonnumeric-seed rejections. Eight numbered-series constructors
reduce to two distinct complete manifests: 10,006 paths plus four paths, all
compared. Their observers leave the legacy list cursor unchanged. FilenameObject
format parsing and Python object identity remain reference-only scope; their
actual file opens use the native explicit-path reader and are compared above.

The observer reads an already-materialized filename list only at original
`close`, before source teardown. It refuses a still-live filename generator.
Iterator frames are captured one actual yield at a time without prefetch.
Eleven independent Python controls verify those boundaries, hook restoration,
missing observer coverage, typed hashes and metadata/index mismatches, stale
output on errors, and rejection of timeouts or unrelated native failures as
successful qualifications. Every expected observer count is checked against
this pinned test set, so silently omitting a test or hook cannot pass.

The report records all ten imported test/runtime source hashes against the
pinned checkout, per-source byte hashes before/after comparison, bridge/helper
and native-source fingerprints, and the unchanged probe binary:
`66aa9b321e841aa4cee5be810666f23475024cd564a83248073268affd7a2fdb`.
The report SHA256 is
`c4a9e3e48c84fb678f145515ec36c09bbaff55fa012695cbcbe9c85197ceb569`.
Fresh codec reads separately demonstrate the legacy EDF negative-index behavior
and the single-frame filename-lookup convention; these are explicitly qualified
against the native nonwrapping cursor policy.

Reproduce with the pinned reference Python environment and the test-only API
probe built from the recorded sources:

```sh
python -m unittest discover -s scripts -p test_series_reference.py
python scripts/series_reference.py --probe target/release/examples/api_probe \
  --fabio-source /tmp/dioptrin-io-fabio-2026.6.0 \
  --report /tmp/series-navigation.json
```
