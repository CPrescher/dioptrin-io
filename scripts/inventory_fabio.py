"""Audit the complete default FabIO registry at an exact source revision.
Usage: python scripts/inventory_fabio.py /path/to/fabio-checkout
No FabIO installation is needed; source is parsed, never imported/executed.
"""
import ast,hashlib,json,pathlib,subprocess,sys
PIN='9ddb6f6ce22e79056bec0f6e6b97d7ebc29cbece'
repo=pathlib.Path(sys.argv[1]);root=pathlib.Path(__file__).resolve().parents[1]
assert subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'],text=True).strip()==PIN
src=repo/'src/fabio'
registry=ast.parse((src/'fabioformats.py').read_text())
codecs=next(ast.literal_eval(n.value) for n in registry.body if isinstance(n,ast.Assign) and any(isinstance(a,ast.Name) and a.id=='_default_codecs' for a in n.targets))
# Audited against each pinned reader implementation. A listed reference branch
# is a coverage target, not a promise that every malformed/input variant works.
DETAILS={
'edfimage':('EDF 0/1/2/3; concatenated frames; general headers; inline/external binary; 2D/volume; integer/float aliases including optional float128','none; internal gzip/zlib/bzip2; optional byte-offset; outer gzip/bzip2','concatenated frames', 'partial: inline 2D, ten scalar types, both byte orders, none/gzip/zlib/bzip2 aliases, optional uncompressed Size and binary padding; general/external/volume/float128/offset pending; outer wrappers via wrapped::Reader'),
'dtrekimage':('d*TREK / ADSC; Rigaku 1.1; endian scalar data and text header','uncompressed','single image; external file series','partial: seven scalar types, both byte orders, nested-brace headers, legacy ADSC TYPE alias, raw padded headers; 1D/volume and permissive missing-type defaults pending'),
'tifimage':('classic TIFF/BigTIFF; endian scalar/color; strip/tile/IFD; photometric/predictor variants','reference TiffIO/Pillow capabilities vary; none/PackBits/LZW/Deflate/JPEG require distinct fixtures','primary IFD pages','partial: ten grayscale BlackIsZero scalar types, both byte orders, IFD stacks, strips/edge tiles, none/PackBits/LZW/Deflate; WhiteIsZero/packed/color/lossy codecs/subIFDs pending'),
'marccdimage':('MAR CCD/Mosaic TIFF pixels plus binary acquisition header','TIFF codecs','inherits TIFF pages','pending detector-specific header parser; generic TIFF pixels only, not validated with MAR files'),
'mar345image':('MAR345 imaging plate / MAR555; supported detector dimensions; overflow table','CCP4/PCK packed and uncompressed branches','single image; file series','pending'),
'fit2dmaskimage':('Fit2D packed bit mask; width alignment and stored orientation','packed bits','single image','implemented bounded little-endian packed masks, aligned/unaligned row widths, raw 1024-byte header and unchanged orientation; same upstream face/click files compared'),
'brukerimage':('Bruker format 86; 8/16/32-bit storage, overflow records, LINEAR header handling','overflow tables / linear encoding','single image; NFRAMES metadata can describe series','pending'),
'bruker100image':('Bruker format 100; base counts, underflow/16/32-bit overflow arrays','underflow/overflow tables','single image','pending'),
'pnmimage':('PBM/PGM/PPM/PAM registry: P1/P2/P5 read branches; P3/P4/P6/P7 explicitly not implemented by pinned reference','ASCII / raw big-endian P5','single image','pending; unsupported reference branches are not counted as working parity targets'),
'GEimage':('GE Angio; standard, EDF-prefixed and blank/ADEPT headers; fixed default dimensions','uncompressed','multiple fixed-size frames','pending'),
'OXDimage':('Oxford Sapphire/KM4CCD; uncompressed/TY1; TY5 reference explicitly marked known broken','none; TY1 differential plus exception arrays; experimental TY5','single image','pending; TY5 needs an independent authority, not sole reliance on the broken reference'),
'dm3image':('DigitalMicrograph DM3 tagged binary structures; nested tags / type codes','uncompressed tagged data','one selected image exposed','pending'),
'HiPiCimage':('Hamamatsu HiPiC IM header, comments and integer pixels','uncompressed','single image','partial: little-endian u16 IM images, raw binary header/comments, parsed UTF8 comments with repeated keys retained in raw bytes; other header/type variants pending'),
'pilatusimage':('Pilatus TIFF + image-description detector header','TIFF codecs','inherits TIFF pages','pending detector-specific header parser; generic TIFF headers retained but no Pilatus fixture'),
'fit2dspreadsheetimage':('Fit2D spreadsheet dimensions and numeric text','ASCII','single image','implemented bounded f32 text rows, dimensions, raw title/comment header; invalid or incomplete numeric rows fail explicitly'),
'kcdimage':('Nonius KappaCCD; header byte type and multiple readouts','uncompressed multiple readouts combined by reference','single resulting image','pending; preserve per-readout data and document any explicit combination'),
'cbfimage':('CBF/imgCIF and miniCBF/Pilatus; signed/unsigned integers 8/16/32/64; raw CIF/MIME headers','x-CBF_BYTE_OFFSET only in pinned FabIO; distinguish terminal32 and extended64 escapes','single 2D image in pinned reference','partial: all eight integer types; explicit Legacy32/Extended64 dialects; MD5; raw CIF headers; outer wrappers via wrapped::Reader. Semantic CIF/Pilatus parsing and multiple sections pending'),
'xsdimage':('XSDataImage XML; base64 binary array; dimension/type descriptors','base64; storage interpretation in XML','single image','pending'),
'binaryimage':('caller-specified dimensions/offset/type/endianness; no autodetection','uncompressed','single explicitly configured image','implemented explicit shape, ten scalar types, both byte orders, absolute/EOF-relative offsets, immutable source checks; no autodetection by design'),
'pixiimage':('Pixirad fixed-frame record header and uint16 data','uncompressed','multiple fixed-size frames','pending'),
'raxisimage':('Rigaku R-AXIS binary/text header, endian data and high-bit dynamic-range flag','high-bit intensity-range encoding','single image','pending; document source u16 flag versus decoded intensity semantics'),
'numpyimage':('NPY array; 2D image / higher-dimensional reshaped frame sequence','NPY binary; do not assume NPZ support','array-leading frame dimension','partial: NPY versions 1/2/3, ten scalar types, both byte orders, C/Fortran arrays, scalar/1D/2D/stack/leading axes; bool/complex/structured/string/float16/float128/empty arrays pending; never executes pickle'),
'eigerimage':('Eiger/Dectris HDF5; direct dataset, grouped data_NNNNNN, master/external links; detector metadata','dataset HDF5 filter pipeline (Bitshuffle/LZ4, gzip etc.)','dataset(s) mapped to frame sequence','partial: explicitly named self-contained 3D u16/u32 dataset; full-file Eiger counts validated. Master discovery/link sequences/metadata/live acquisition pending'),
'hdf5image':('explicit file::dataset HDF5 image or stack, nested frame selection','native HDF5 filters','2D/3D datasets','partial: explicit 3D u16/u32 dataset; 2D/signed/float/generic headers pending. VDS rejected; raw snapshot also rejects indirect/missing/partial chunks'),
'fit2dimage':('Fit2D binary $FFF_START records and endian numeric arrays','uncompressed records','single image','pending'),
'speimage':('Princeton SPE versions 2/3; binary header and v3 XML metadata/ROI','uncompressed typed pixels','frame selection (no generic getframe override)','pending'),
'jpegimage':('JPEG/JFIF/Exif via Pillow; lossy source samples must retain decoded values and provenance','JPEG','single image','pending'),
'jpeg2kimage':('JPEG2000 codestream/JP2/JPX via optional Glymur/Pillow','JPEG2000 lossless/lossy require separate fixtures','single image','pending'),
'mpaimage':('MPA multiwire; text configuration + spectrum/data blocks','ASCII or binary branches per source','single image','pending'),
'mrcimage':('MRC/CCP4/MAP/FEI; endian header, supported mode codes and extended header','uncompressed typed arrays','NZ image stack','pending'),
'esperantoimage':('CrysAlisPro Esperanto; ASCII header, int32 image, raw or compressed format selector','4-byte integers / AGI bitfield','single image','pending'),
'limaimage':('LImA HDF5 creator/default path discovery and acquisition metadata','HDF5 filters','3D detector dataset','pending discovery/metadata fixtures; explicit compatible dataset can use general HDF5 API'),
'lambdaimage':('Lambda HDF5/NeXus detector-description/path convention','HDF5 filters','3D detector dataset','pending discovery/metadata fixtures; explicit compatible dataset can use general HDF5 API'),
'adscimage':('compatibility alias to DtrekImage (same registered codec identity)','same as dtrekimage','same as dtrekimage','implemented via native::Format::Dtrek with legacy TYPE=unsigned_short; same decoder and remaining gaps as dtrekimage'),
'sparseimage':('pyFAI/LImA sparse Bragg HDF5: frame pointers/index/intensity/background/noise; reconstruction policy matters','HDF5 filters plus sparse/background representation','sparse frame sequence','pending; no stochastic or invented dense counts by default'),
'xcaliburimage':('Xcalibur CCD/masked-pixel binary structs and masks','binary records','single mask/image exposed','pending'),
}
fixtures={
'edfimage':['tests/data/edf/*.edf','tests/binary.rs'],
'cbfimage':['tests/data/cbf/*.cbf','tests/binary.rs'],
'tifimage':['tests/data/tiff/*.tif','tests/tiff.rs'],
'eigerimage':['tests/data/counts-bitshuffle-lz4.h5','tests/reader.rs','tests/raw.rs','docs/benchmarks/2026-11-05-native-eiger/count-reference.json'],
'hdf5image':['tests/reader.rs','tests/raw.rs','tests/cache.rs'],
}
fixtures.update({module:['tests/data/native/provenance.json','tests/native.rs','tests/wrapped.rs','docs/FABIO_PARITY.md'] for module in ['dtrekimage','adscimage','fit2dmaskimage','HiPiCimage','fit2dspreadsheetimage','binaryimage','numpyimage']})
rows=[]
for module,cls in codecs:
 path=src/(module+'.py');data=path.read_bytes();t=ast.parse(data)
 c=next((n for n in t.body if isinstance(n,ast.ClassDef) and n.name==cls),None)
 extensions=[]
 if c:
  extensions=next((ast.literal_eval(n.value) for n in c.body if isinstance(n,ast.Assign) and any(isinstance(a,ast.Name) and a.id=='DEFAULT_EXTENSIONS' for a in n.targets)),[])
 variants,compression,stack,status=DETAILS[module]
 rows.append({'module':module,'class':cls,'extensions':extensions,'source':f'https://github.com/silx-kit/fabio/blob/{PIN}/src/fabio/{module}.py','source_sha256':hashlib.sha256(data).hexdigest(),'subformats_and_types':variants,'compression':compression,'stack':stack,'implementation':status,'validation':fixtures.get(module,[]),'parity_verified':False})
out={'reference':{'package':'FabIO','version':'2026.6.0','tag':'v2026.06','commit':PIN,'registry_entries':len(codecs),'unique_registered_codecs':35,'registry_source':f'https://github.com/silx-kit/fabio/blob/{PIN}/src/fabio/fabioformats.py','registry_sha256':hashlib.sha256((src/'fabioformats.py').read_bytes()).hexdigest()},'scope':'All default readers, aliases, explicit raw input, format discovery, source types/headers, subformats, compression and stack semantics. No format-level parity claim without independent fixtures. Write/conversion APIs are out of scope.','cross_cutting_targets':{'outer_file_compression':{'reference':['gzip (.gz)','bzip2 (.bz2)'],'implementation':'optional wrappers feature: magic-detected gzip/bzip2, bounded temporary staging, original-source checks; native/EDF/CBF/TIFF; HDF5 and direct file-like APIs pending','source':f'https://github.com/silx-kit/fabio/blob/{PIN}/src/fabio/fabioutils.py'},'file_series':'pending explicit series API; numbered neighboring files must not be silently substituted','autodetection':'pending; current backends are explicitly selected by caller','orientation':'return stored rows/columns and metadata; consumer transformations are explicit','source_mutation':'immutable snapshots check normal filesystem changes; no adversarial metadata-preserving writes or live refresh','header_coverage':'raw EDF/CBF/native headers, parsed native fields and typed TIFF tags; HDF5 finite scalar API only; complete semantic detector header maps pending'},'formats':rows}
(root/'docs/fabio-2026.6.0-inventory.json').write_text(json.dumps(out,indent=2)+'\n')
lines=['# FabIO reading coverage target','',f'Reference: [FabIO 2026.6.0, tag v2026.06](https://github.com/silx-kit/fabio/tree/{PIN}), commit `{PIN}`. The pinned registry has 36 entries and 35 unique codecs; ADSC aliases d*TREK. This inventory retains **every reader**, including raw input and masks. It is a target inventory, not a parity claim.','', 'Machine-readable subformats, compression, stacks, provenance and validation are in [the inventory](fabio-2026.6.0-inventory.json). Regenerate with `python scripts/inventory_fabio.py /path/to/pinned/fabio`.','', '| Reader | Subformats / source types | Compression | Stack | Current status |','|---|---|---|---|---|']
for row in rows:
 lines.append('| '+' | '.join([f'[{row["module"]}]({row["source"]})',row['subformats_and_types'],row['compression'],row['stack'],row['implementation']])+' |')
lines+=['','Outer gzip/bzip2 wrappers are supported by the optional wrapped::Reader for native/EDF/CBF/TIFF. Content autodetection, HDF5 wrappers and neighboring-file series remain separate pending targets. A TIFF-based detector is not marked supported merely because a generic TIFF can be decoded. Likewise, explicit HDF5 dataset reading does not establish Eiger-master/LImA/Lambda/sparse reconstruction parity.','', 'The pinned PNM reader explicitly rejects P3/P4/P6/P7; its P1/P2 loops do not advance the row index, which is recorded as a reference behavior difference, and its Oxford TY5 branch is marked known broken. Those require independent evidence before treating them as working reference implementations. Sparse reconstructions and R-AXIS dynamic-range encoding also need explicit data/interpretation contracts.','', 'Continue through every remaining row, using the pinned upstream codec suite and its exact files as an acceptance corpus (see FABIO_PARITY.md). CBF/EDF semantic/external headers, HDF5 typed/master sources, remaining codecs, full metadata comparisons and discovery all remain part of the target. No PhaseSmith migration or Dioptas changes are part of this branch.','']
(root/'docs/FORMAT_COVERAGE.md').write_text('\n'.join(lines))
