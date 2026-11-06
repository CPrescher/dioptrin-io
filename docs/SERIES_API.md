# Frozen native file series

The `wrappers` feature exposes `series::Series<O>` over existing
`wrapped::Reader`s. It concatenates files in caller order using their actual
frame counts. A file with three frames followed by files with one and two frames
has global ranges `0..3`, `3..4`, and `4..6`. `locate` returns the original global
index, file index, local frame index and absolute supplied path. `files` exposes
each file's path, first global frame and exact frame count. No shape, dtype,
channels, count, reserved value or orientation is converted across files.

```rust
use dioptrin_io::{native, series, tiff, wrapped};
use std::path::Path;

fn open_edf(path: &Path) -> Result<wrapped::Reader, wrapped::Error> {
    wrapped::Reader::open(
        path, wrapped::Format::Edf, wrapped::Limits::default(),
        native::Limits::default(), tiff::Limits::default(),
    )
}

# fn example() -> Result<(), wrapped::Error> {
let mut series = series::Series::open(
    ["scan_0001.edf", "scan_0002.edf"], open_edf,
    series::Limits::default(),
)?;
let mut output = wrapped::FrameBuffer::default();
series.read_frame_into(0, &mut output)?;
let location = series.current_location();
let original_metadata = series.current_metadata_ref()?;
# Ok(())
# }
```

The opener must be a deterministic configuration: it must open the supplied
path, choosing the same format, interpretations, external-link policy and
backend/transport limits on every invocation. A mixed-format series can use an
immutable per-path configuration. Rust's `Fn` bound permits interior mutability;
changing that configuration violates this contract. Runtime checks reject a
substituted source, changed graph/bindings or changed frame count, but do not
prove that an arbitrary callback retained the same interpretation of unchanged
bytes.

Indexing opens one file at a time and drops its reader after retaining source
snapshots, path bindings and frame ranges. Each frame operation first drops the
previous reader, checks every indexed source (including inactive external
graphs), reopens the requested file, compares its graph and count with the
manifest, reads the requested local frame, and checks sources again. A single
last-successful reader is retained for borrowed metadata; there is no decoded
cache per indexed file and no OS handle per manifest entry. `Limits` bounds
file count, cumulative frames and conservatively accounted path/record metadata;
the opener separately bounds each backend, transport and external graph.

Original filename bindings remain distinct from resolved paths, so symlink
retargeting, replacement, removal and growth invalidate the series even when an
old target survives. This uses the existing immutable filesystem snapshot
identity/timestamp contract, not content hashing. Every failed frame read clears
visible output and current metadata/location. Source, opening and decoding
failures permanently invalidate the series; an out-of-range index clears output
without invalidating its manifest. Previously returned caller-owned frames are
snapshots of their successful reads, not live filesystem views.

`current_metadata_ref` checks the source graph without reopening or changing a
navigation cursor's last-successful state. It returns errors for changed sources.
The mutable `current_metadata` additionally invalidates the series on a failed
check. `with_metadata(index, callback)` supports inspection before decoding and
cannot return borrowed metadata from a dropped reader. It clears the retained
current reader and checks the graph after the callback. `Series` implements
`navigation::FrameSource`, so `navigation::Cursor` traverses exact file boundaries
using the same zero-based global indices.

`Numbered` is an explicit template: `stem`, `suffix`, nonnegative `first`, positive
`step`, and minimum decimal width `digits` (0 through 20). `path(number)` formats
one name and allows width rollover. `through(last, limits)` generates an inclusive
range without checking existence; opening an explicit manifest with any missing
path fails. `until_missing(limits)` freezes absolute paths up to the first absent
directory entry and records that `stopped_at` path. A broken symlink, unreadable
file, nonregular file, numeric overflow, or a bound reached before the first
missing path is an error. Later files do not extend a frozen manifest. Empty
manifests are allowed; indexed files with zero frames are rejected.

`Numbered::from_seed(path, step)` provides the conventional filename adapter.
It excludes `.gz`/`.bz2` and a nonnumeric format extension before selecting the
last numeric run in the UTF-8 basename. It preserves the supplied directory,
padding, post-number text and original suffix. Thus `mydata001.mar2300.gz` steps
to `mydata002.mar2300.gz`, and `image.0123` steps to `image.0124`. Names without
a number and overflowing u64 numbers fail. Explicit templates support arbitrary
non-UTF-8 filenames and conventions beyond this adapter; no codec registry or
extension-based decoding participates in series construction.

The intended ordered-list, actual multiframe, inclusive numbered-range and
first-missing behavior was checked against pinned FabIO commit
`9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece`, `file_series.py`,
`test_file_series.py`, `test_filename_steps.py`, and `test_filenames.py`.
The native layer does not adopt legacy helpers' skipped failed images, implicit
negative numbering, format guesses, or fixed-frame-count shortcuts. Independent
Rust tests compare every frame with the existing direct reader, exercise mixed
shape/dtype and outer gzip/bzip2 stacks, navigation edges, bounded number
generation, frozen growth, missing/renamed sources, mutation around open and
metadata callbacks, and inactive HDF5 duplicate-target alias retargeting.
