# Explicit finite HDF5 printf mappings

`hdf5::typed::Reader::open_snapshot_with_vds` reads a caller-approved finite prefix of a stored HDF5 virtual mapping through the existing native typed source decoder. It does not ask HDF5 to discover filenames, choose an available-source extent, or read a VDS with implicit missing-source fill.

`VdsOptions::printf_mappings` contains `PrintfMapping` policies. Each identifies the actual owner file (`source_file: None` for the input file, `Some(path)` for an external nested owner), resolved absolute owner dataset, original DCPL mapping index, exact stored filename pattern, exact stored dataset pattern, and positive `blocks`. All six fields matter. The approved blocks are exactly `0..blocks`. Duplicate, conflicting and unused plans fail. Source aliases remain snapshot-checked; retargeting an owner symlink invalidates the reader.

The supported grammar is the one implemented by native HDF5 2.2.0 and the independent reference HDF5 1.14.6: `%b` is the decimal zero-based unlimited-axis block index; `%%` is one literal percent. Repeated substitutions use the same block index in both names. Expansion occurs once: `%%b` becomes literal `%b`. Width, zero-padding, axis-qualified tokens such as `%0b`, and other printf conversions are rejected. Some published HDF5 documentation describes additional syntax; these tested producer versions reject it. The proof fingerprints the primary native `H5Dvirtual.c` implementation and the reference extension/library binaries.

Filename bindings name the expanded literal strings exactly, including literal percent signs. No basename search, environment expansion or directory enumeration occurs. Filename `.` refers to the current mapping owner file, including a nested owner; it needs no filename binding. A root-file dataset-only pattern therefore works with `Input::from_bytes` without an origin or knowledge of its private capture filename.

The unlimited-axis effective extent ends at the final approved block. A stored dataspace extent of 100 and a three-block prefix ending at 6 remain distinct: original extent 100 is retained in provenance, while the read view has extent 6. This is an explicitly selected prefix view. Fixed axes retain their declared extents. All mappings must cover their original selected positions within the final combined domain: a shorter prefix cannot silently become declared fill because another mapping extends that domain. Staggered three-block/two-block interleaves are valid when both original patterns cover the resulting five-frame domain completely.

Each concrete mapping retains `PrintfExpansion { declaration: Arc<PrintfDeclaration>, block_index }`. The declaration contains the original physical owner, mapping index, pattern strings, source/virtual selection records and selected policy. Effective finite selections remain separately exposed. Typed words—including noncanonical Boolean bytes, float16/complex NaN payloads, endian declarations and uint64 values above the floating-point exact range—follow the existing source contract.

Name lengths, block counts, shape/products, metadata, frame bytes and HDF5 selection cardinalities are checked before relevant allocations. Expansion copies native dataspaces with `H5Scopy`; it does not mutate aliased declarations. Concrete sources must exist and have allocated mapped storage. Explicit declared fill applies only to genuinely unmapped regions. Every graph source is checked before and after reads, including sources not used by the selected frame; replacement, growth and owner-alias retargeting invalidate output. Failed reads clear visible pixels and shape. New files outside the approved prefix are not added to an existing reader.

The CC0 fixtures in `tests/data/vds-printf` contain 14 main cases and 60 frames, plus independent mixed-prefix rejection and staggered-prefix controls. Expected bytes are assembled from authored scalar source words, never from a VDS read. Thirteen cases match unchanged pinned FabIO and h5py reads directly. The nested physical-owner case qualifies a narrower reference discrepancy: fresh HDF5 1.14.6/FabIO reads return precisely declared zero fill; in a separate process, keeping the independently verified child open makes the unchanged outer read return every authored word. The primary reference is never warmed or patched to claim direct equality. This qualification establishes the observed pinned cache-order behavior; it is not a claim about all HDF5 releases or all nested mappings.

The machine proof is [the scoped report](vds/2026-11-06-vds-advanced-ojpeg-printf.json). Reproduce with:

```sh
/tmp/dioptrin-io-fabio-suite-env/bin/python scripts/vds_printf_reference.py \
  --probe target/release/examples/api_probe \
  --fabio-source /tmp/dioptrin-io-fabio-2026.6.0 \
  --report docs/vds/2026-11-06-vds-advanced-ojpeg-printf.json
```

The implementation uses existing HDF5 selection APIs and existing source codecs; it adds no parallel decoder or dependency. Existing HDF5/native codec license notices apply. The authored fixture generator, fixture arrays and expected words are CC0-1.0; Python, h5py and pinned FabIO are test-only references. No throughput claim is made.
