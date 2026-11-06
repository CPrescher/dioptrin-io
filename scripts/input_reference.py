"""Test-only same-source content discovery and genuine stdin qualification.

No reference object is mutated and no reader is retried. Whole native JSON records
are compared, including component/view hashes and integrity reports. The sole
permitted image-record normalization is an exact path from the probe's explicit
stderr input_transport manifest; every replacement is retained in the evidence.
"""
import hashlib
import importlib
import inspect
import io
import json
import pathlib
import subprocess

CODECS = {
    'numpyimage': 'Npy', 'dtrekimage': 'Dtrek', 'fit2dmaskimage': 'Fit2dMask',
    'HiPiCimage': 'HiPic', 'pnmimage': 'Pnm', 'fit2dspreadsheetimage': 'Fit2dSpreadsheet',
    'edfimage': 'Edf', 'cbfimage': 'Cbf', 'tifimage': 'Tiff', 'marccdimage': 'Tiff',
    'pilatusimage': 'Tiff', 'jpegimage': 'Jpeg', 'jpeg2kimage': 'Jpeg2000',
    'brukerimage': 'Bruker', 'bruker100image': 'Bruker', 'OXDimage': 'Oxford',
    'raxisimage': 'Raxis', 'GEimage': 'Ge', 'pixiimage': 'Pixi', 'speimage': 'Spe',
    'mrcimage': 'Mrc', 'dm3image': 'Dm3', 'fit2dimage': 'Fit2d', 'kcdimage': 'Kcd',
    'mpaimage': 'Mpa', 'esperantoimage': 'Esperanto', 'mar345image': 'Mar345',
    'xsdimage': 'Xsd', 'hdf5image': 'Hdf5', 'eigerimage': 'Eiger',
    'limaimage': 'Lima', 'lambdaimage': 'Lambda', 'sparseimage': 'Sparse',
    'xcaliburimage': 'Xcalibur',
}
HDF5 = {'Hdf5', 'Eiger', 'Lima', 'Lambda', 'Sparse'}


def fingerprint(path):
    path = pathlib.Path(path)
    with path.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    return {'path': str(path.resolve()), 'bytes': path.stat().st_size, 'sha256': digest}


def reference_sources(pinned_source):
    """Actual imported routing code must equal the pinned checkout, not its name."""
    sources = {}
    for name in ('openimage', 'fabioimage', 'fabioutils'):
        module = importlib.import_module('fabio.' + name)
        actual = pathlib.Path(inspect.getfile(module))
        pinned = pathlib.Path(pinned_source) / 'src/fabio' / (name + '.py')
        actual_fingerprint, pinned_fingerprint = fingerprint(actual), fingerprint(pinned)
        sources[name] = {'actual': actual_fingerprint, 'pinned': pinned_fingerprint,
                         'matches_pinned_source': actual_fingerprint['sha256'] == pinned_fingerprint['sha256']}
    return {'modules': sources, 'matches_pinned_source': all(item['matches_pinned_source'] for item in sources.values())}


def routing_decisions(path, argument):
    """Observe real _openimage choices on fresh path and BytesIO inputs, no .read."""
    module = importlib.import_module('fabio.openimage')
    from fabio.fabioimage import FabioImage
    def decision(value):
        try:
            image = module._openimage(value)
            return {'status': 'selected', 'codec': type(image).__module__.split('.')[-1],
                    'class': type(image).__name__}
        except Exception as error:
            return {'status': 'rejected', 'error_type': type(error).__name__, 'error': str(error)}
    result = {'path': decision(argument)}
    # This BytesIO is independent of the upstream object and its stream cursor.
    result['bytes_io'] = decision(io.BytesIO(pathlib.Path(path).read_bytes()))
    try:
        with FabioImage()._open(str(path), 'rb') as stream:
            prefix = stream.read(18)
        result['unwrapped_prefix_hex'] = prefix.hex()
        for key, filename in (('magic_without_filename', ''), ('magic_with_filename', argument)):
            try:
                result[key] = {'status': 'selected', 'format': module._do_magic(prefix, filename)}
            except Exception as error:
                result[key] = {'status': 'rejected', 'error_type': type(error).__name__, 'error': str(error)}
    except Exception as error:
        result['prefix_error'] = {'error_type': type(error).__name__, 'error': str(error)}
    return result


def reference_alignment(reference, expected, selected, source_container_alias=False):
    """Describe reference routing differences; explicit reader count proof is separate."""
    result = {}
    for api in ('path', 'bytes_io'):
        route = reference[api]
        detected = CODECS.get(route.get('codec'))
        if expected is None:
            status = 'explicit_raw_configuration'
        elif route['status'] == 'rejected':
            status = ('reference_stream_limitation' if api == 'bytes_io' and reference['path']['status'] == 'selected'
                      else 'reference_content_routing_rejected')
        elif source_container_alias and detected == selected:
            status = 'source_container_without_filename_override'
        elif detected == expected:
            status = 'same_codec_family'
        elif detected == 'Hdf5' and expected in HDF5 or expected == 'Hdf5' and detected in HDF5:
            status = 'container_routing_alias'
        else:
            status = ('stronger_native_content_decision' if selected == expected else 'reference_routing_differs_from_explicit_reader')
        result[api] = {'status': status, 'native_content_codec': selected,
                       'explicit_reader_codec': expected, 'reference_routed_codec': detected}
    if reference['path']['status'] == 'selected':
        named, unnamed = reference.get('magic_with_filename', {}), reference.get('magic_without_filename', {})
        if named.get('status') == 'rejected':
            result['path']['filename_fallback'] = True
        elif unnamed.get('status') == 'rejected' and named.get('status') == 'selected':
            result['path']['path_container_metadata_dependency'] = True
        elif named.get('format') != unnamed.get('format'):
            result['path']['filename_convention'] = True
    return result


def parse_records(stdout):
    return [json.loads(line) for line in stdout.splitlines() if line.strip()]


def transport_manifest(stderr):
    manifests = []
    for line in stderr.splitlines():
        try:
            value = json.loads(line)
        except (ValueError, TypeError):
            continue
        if isinstance(value, dict) and 'input_transport' in value:
            manifests.append(value['input_transport'])
    if len(manifests) != 1:
        raise ValueError(f'expected one explicit input_transport manifest, found {len(manifests)}')
    manifest = manifests[0]
    roots = manifest.get('captured_roots', [])
    if manifest.get('captured_root'):
        roots = [manifest['captured_root'], *roots]
    if not roots or any(not isinstance(root, str) or not pathlib.Path(root).is_absolute() for root in roots):
        raise ValueError('input_transport must declare absolute captured root paths')
    if not isinstance(manifest.get('original_argument'), str):
        raise ValueError('input_transport must retain the exact original argument')
    return manifest, list(dict.fromkeys(roots))


def normalize_transport(value, roots, original_root):
    """Never basename-match, rewrite prefixes, or remove arbitrary metadata fields."""
    replacements = []
    def visit(item, location):
        if isinstance(item, str) and item in roots:
            replacements.append({'field': location, 'captured_root': item, 'original_root': original_root})
            return original_root
        if isinstance(item, list):
            return [visit(child, f'{location}[{index}]') for index, child in enumerate(item)]
        if isinstance(item, dict):
            return {key: visit(child, f'{location}.{key}') for key, child in item.items()}
        return item
    return visit(value, '$'), replacements


def differences(actual, expected, limit=30):
    """Exact recursive equality, including key absence, order and JSON scalar types."""
    found = []
    def visit(left, right, location):
        if len(found) >= limit:
            return
        if type(left) is not type(right):
            found.append({'field': location, 'actual': left, 'expected': right})
        elif isinstance(left, dict):
            for key in sorted(left.keys() | right.keys()):
                if key not in left or key not in right:
                    found.append({'field': location + '.' + key, 'actual': left.get(key),
                                  'expected': right.get(key), 'missing_in': 'actual' if key not in left else 'expected'})
                else:
                    visit(left[key], right[key], location + '.' + key)
        elif isinstance(left, list):
            if len(left) != len(right):
                found.append({'field': location + '.length', 'actual': len(left), 'expected': len(right)})
            for index, (a, b) in enumerate(zip(left, right)):
                visit(a, b, f'{location}[{index}]')
        elif left != right:
            found.append({'field': location, 'actual': left, 'expected': right})
    visit(actual, expected, '$')
    return found


def _run(command, path, timeout, streaming=False):
    if streaming:
        with pathlib.Path(path).open('rb') as stream:
            process = subprocess.run(command, stdin=stream, capture_output=True, text=True, timeout=timeout)
    else:
        process = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    return process


def discovery_classification(codec, detection, frames=()):
    expected = CODECS.get(codec)
    selected = detection.get('selected')
    candidates = detection.get('candidates')
    if not isinstance(candidates, list):
        raise ValueError('discovery JSON has no candidates array')
    if selected is not None:
        matches = [candidate for candidate in candidates if candidate.get('codec') == selected]
        if len(matches) != 1 or matches[0].get('evidence') not in {'Signature', 'StructuredHeader'} or not matches[0].get('available') or detection.get('resolution_error') is not None:
            raise ValueError('discovery selected a codec without unique available positive evidence')
    if codec == 'binaryimage':
        return 'explicit_configuration_required'
    if expected is None:
        return 'unknown_explicit_codec'
    if selected == expected:
        return 'match'
    if codec == 'GEimage' and selected == 'Edf' and frames and all(
            frame.get('metadata_details', {}).get('kind') == 'ApsEdf8192'
            and frame['metadata_details'].get('edf_fields') for frame in frames):
        return 'source_qualified_container_alias'
    if selected == 'Hdf5' and expected in HDF5 or expected == 'Hdf5' and selected in HDF5:
        return 'container_alias'
    if selected is None and len(candidates) == 1 and candidates[0].get('codec') == expected and expected in {'Ge', 'Fit2dSpreadsheet'} and candidates[0].get('evidence') == 'AmbiguousHeader' and detection.get('resolution_error'):
        return 'conservative_ambiguity'
    return 'difference'


def outer_integrity_evidence(frames):
    reports = [frame['source_recovery']['source_report'] for frame in frames
               if 'source_recovery' in frame and 'source_report' in frame['source_recovery']]
    return [report for report in reports if report.get('compression') != 'None'
            and report.get('integrity') in {'UnverifiedTruncated', 'ChecksumFailure', 'DecodeFailure'}]


def qualify(probe, codec, adapter, path, argument, extra, original_stdout, timeout):
    """Run while generated sources exist, before upstream teardown; errors fail gate."""
    path = pathlib.Path(path)
    result = {'status': 'error', 'adapter': adapter, 'source': fingerprint(path),
              'stream_contract': 'real subprocess stdin file handle; explicit origin; identical adapter/arguments; complete JSON record comparison',
              'reference_explicit_codec': codec}
    try:
        frames = parse_records(original_stdout)
        result['qualification_scope'] = ('metadata_only' if frames and all(frame.get('header_only') is True for frame in frames) else 'complete_native_records')
        origin = str(path.resolve().parent)
        stream_command = [str(probe), adapter, argument, *extra, '--stream', '--stream-origin', origin]
        streamed = _run(stream_command, path, timeout, streaming=True)
        stream = {'exit_code': streamed.returncode, 'arguments': stream_command[1:], 'origin_directory': origin}
        result['native_stream'] = stream
        if streamed.returncode:
            stream.update(status='rejected', error=streamed.stderr[:4000])
        else:
            manifest, roots = transport_manifest(streamed.stderr)
            if manifest['original_argument'] != argument:
                raise ValueError('stream transport manifest changed the original argument')
            normalized, replacements = normalize_transport(parse_records(streamed.stdout), roots, str(path.resolve()))
            delta = differences(normalized, frames)
            stream.update(status='match' if not delta else 'difference', transport=manifest,
                          path_replacements=replacements, record_count=len(normalized), differences=delta,
                          stdout_sha256=hashlib.sha256(streamed.stdout.encode()).hexdigest(),
                          original_stdout_sha256=hashlib.sha256(original_stdout.encode()).hexdigest())
        reference = routing_decisions(path, argument)
        discovered = _run([str(probe), 'discover', str(path)], path, timeout)
        discovered_stream = _run([str(probe), 'discover', str(path), '--stream', '--stream-origin', origin], path, timeout, streaming=True)
        discovery = {'path': {'exit_code': discovered.returncode}, 'stream': {'exit_code': discovered_stream.returncode},
                     'reference': reference}
        result['discovery'] = discovery
        if discovered.returncode or discovered_stream.returncode:
            for output, process in ((discovery['path'], discovered), (discovery['stream'], discovered_stream)):
                output['error'] = process.stderr[:4000]
                if not process.returncode:
                    output['result'] = json.loads(process.stdout)
            evidence = outer_integrity_evidence(frames)
            if discovered.returncode == 2 and discovered_stream.returncode == 2 and evidence:
                discovery.update(status='qualified_outer_integrity_rejection', outer_integrity=evidence,
                                 qualification='strict transport discovery cannot verify this outer container; recovered sample availability is a separate policy, not a discovery/count success')
            else:
                discovery['status'] = 'rejected'
        else:
            path_detection, stream_detection = json.loads(discovered.stdout), json.loads(discovered_stream.stdout)
            discovery['path']['result'], discovery['stream']['result'] = path_detection, stream_detection
            # Stream/origin flags are transport evidence, never removed from the
            # image comparison. Discovery's content decision must be identical.
            keys = ('candidates', 'selected', 'resolution_error')
            delta = differences({key: stream_detection[key] for key in keys}, {key: path_detection[key] for key in keys})
            path_provenance, stream_provenance = path_detection['provenance'], stream_detection['provenance']
            provenance_delta = differences({key: stream_provenance[key] for key in ('encoded_bytes', 'decoded_bytes', 'compression')},
                                           {key: path_provenance[key] for key in ('encoded_bytes', 'decoded_bytes', 'compression')})
            if path_provenance.get('stream') is not False or stream_provenance.get('stream') is not True or stream_provenance.get('origin_directory') != origin:
                provenance_delta.append({'field': 'discovery.provenance', 'error': 'stream flag or explicit origin was not retained'})
            status = discovery_classification(codec, path_detection, frames)
            if status == "source_qualified_container_alias":
                discovery["classification_evidence"] = {"native_reader":"ge", "source_header_kind":"ApsEdf8192",
                    "frame_count":len(frames), "edf_fields":frames[0]["metadata_details"]["edf_fields"],
                    "qualification":"content identifies the EDF header; GE fixed-frame interpretation remains an explicit reader choice, qualified by its successful source metadata"}
            discovery.update(status='difference' if delta or provenance_delta else status,
                             content_differences=delta, provenance_differences=provenance_delta,
                             reference_alignment=reference_alignment(reference, CODECS.get(codec), path_detection.get('selected'), status == 'source_qualified_container_alias'))
        result['source_after'] = fingerprint(path)
        result['source_unchanged'] = result['source'] == result['source_after']
        accepted = {'match', 'container_alias', 'source_qualified_container_alias', 'conservative_ambiguity', 'explicit_configuration_required', 'qualified_outer_integrity_rejection'}
        result['status'] = 'qualified' if stream.get('status') == 'match' and discovery.get('status') in accepted and result['source_unchanged'] else 'difference'
    except Exception as error:
        result['error'] = f'{type(error).__name__}: {error}'
    return result


def summarize(reads):
    import collections
    events = [record.get('input_apis', {}) for record in reads]
    return {'read_events': len(events), 'status': dict(collections.Counter(item.get('status', 'not_compared') for item in events)),
            'native_stream': dict(collections.Counter(item.get('native_stream', {}).get('status', 'not_compared') for item in events)),
            'discovery': dict(collections.Counter(item.get('discovery', {}).get('status', 'not_compared') for item in events)),
            'scopes': dict(collections.Counter(item.get('qualification_scope', 'not_compared') for item in events)),
            'reference_data_getter_errors': sum('reference_data_getter_error' in record for record in reads),
            'all_events_qualified': bool(events) and all(item.get('status') == 'qualified' for item in events)}


class HeaderBridge:
    """Observe EDF .readheader separately; never access data or alter its result."""
    def __init__(self, probe, timeout, report, current_test):
        self.probe, self.timeout, self.report, self.current_test = probe, timeout, report, current_test
        self.report['input_header_apis'] = []
        self.active = False

    def install(self):
        from fabio.edfimage import EdfImage
        import functools
        original = EdfImage.readheader
        @functools.wraps(original)
        def readheader(image, source, *args, **kwargs):
            if self.active:
                return original(image, source, *args, **kwargs)
            self.active = True
            captured = None
            try:
                if hasattr(source, 'read') and hasattr(source, 'seek'):
                    position = source.tell()
                    source.seek(0)
                    captured = source.read()
                    source.seek(position)
                # Exceptions retain their original semantics and traceback.
                result = original(image, source, *args, **kwargs)
                record = {'test': self.current_test(), 'codec': 'edfimage', 'operation': 'readheader',
                          'source': str(source) if isinstance(source, (str, pathlib.Path)) else type(source).__name__,
                          'status': 'error', 'scope': 'headers only; no pixel getter invoked or count claim'}
                self.report['input_header_apis'].append(record)
                try:
                    self.compare(record, image, source, captured)
                except Exception as error:
                    record['error'] = f'{type(error).__name__}: {error}'
                return result
            finally:
                self.active = False
        EdfImage.readheader = readheader

    def compare(self, record, image, source, captured):
        import tempfile
        from metadata_parity import compare, encode
        with tempfile.TemporaryDirectory(prefix='dioptrin-edf-header-api-') as directory:
            if captured is not None:
                path = pathlib.Path(directory) / 'stream.bin'
                path.write_bytes(captured)
                record['legacy_reference_stream_materialized'] = True
            elif isinstance(source, (str, pathlib.Path)):
                path = pathlib.Path(source)
            else:
                raise ValueError('unsupported header-only source')
            record['source_fingerprint'] = fingerprint(path)
            native = _run([str(self.probe), 'edf-header', str(path)], path, self.timeout)
            if native.returncode:
                record.update(status='native_rejected', error=native.stderr[:4000], exit_code=native.returncode)
                return
            frames = parse_records(native.stdout)
            record['native_headers'] = frames
            references = [encode(dict(frame.header)) for frame in image._frames]
            record['reference_headers'] = references
            if len(frames) != len(references):
                record.update(status='difference', error='native/reference local header count differs')
                return
            if not frames or any(frame.get('header_only') is not True or frame.get('pixels_available') is not False for frame in frames):
                raise ValueError('native header API did not explicitly preserve unavailable pixels')
            record['metadata'] = [{'frame': index, **compare(reference, frame, 'edfimage')}
                                  for index, (reference, frame) in enumerate(zip(references, frames))]
            # Complete header records (including original prefix and general
            # records) must survive the actual stdin transport unchanged.
            record['input_apis'] = qualify(self.probe, 'edfimage', 'edf-header', path, str(path), [], native.stdout, self.timeout)
            record['status'] = 'qualified' if all(item['status'] in {'match', 'reference_empty'} for item in record['metadata']) and record['input_apis']['status'] == 'qualified' else 'difference'
