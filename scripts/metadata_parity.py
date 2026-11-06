"""Typed test-only metadata comparison. Original values and differences survive.

This module never writes source files or changes pixel comparisons. Raw text,
binary values and parsed numbers remain distinguishable; no blanket numeric
coercion or unit conversion is allowed.
"""
import hashlib
import math

import numpy as np


def encode(value):
    if isinstance(value, np.ndarray):
        data = np.ascontiguousarray(value.astype(value.dtype.newbyteorder("<"), copy=False))
        result = {"kind": "array", "dtype": value.dtype.name, "shape": list(value.shape),
                  "sha256": hashlib.sha256(data.tobytes()).hexdigest()}
        if value.size <= 256:
            result["values"] = encode(value.tolist())
        return result
    if isinstance(value, np.generic):
        return encode(value.item())
    if isinstance(value, bytes):
        result = {"kind": "bytes", "length": len(value), "sha256": hashlib.sha256(value).hexdigest()}
        if len(value) <= 4096:
            result["hex"] = value.hex()
        return result
    if isinstance(value, dict):
        return {str(key): encode(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [encode(item) for item in value]
    if isinstance(value, float) and not math.isfinite(value):
        return {"kind": "float", "value": "nan" if math.isnan(value) else "inf" if value > 0 else "-inf"}
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, np.dtype) or isinstance(value, type) and issubclass(value, np.generic):
        return {"kind": "dtype", "value": np.dtype(value).str}
    return {"kind": "unsupported", "python_type": type(value).__qualname__, "repr": repr(value)}


def _array(values, dtype, shape=None):
    array = np.asarray(values, dtype=dtype)
    return encode(array.reshape(shape) if shape is not None else array)


def project(codec, frame, backend=None):
    """Explicit reference presentation from native metadata only.

    This function never sees reference values. Source metadata stays unchanged;
    each alias, type conversion, derived unit and synthetic default is named.
    Unimplemented projections leave real differences visible.
    """
    native = frame.get("fields", {})
    details = frame.get("metadata_details", {})
    view, mappings = dict(native), {}

    def put(key, value, source, kind="representation"):
        view[key] = value
        mappings[key] = {"source": source, "kind": kind}

    if codec == "edfimage":
        # Native normalizes case as required by EDF, retaining original bytes.
        view = {key.upper(): value for key, value in native.items()}
    elif codec in {"brukerimage", "bruker100image"}:
        if "HDRBLKS" in native:
            put("HDRBLKS", int(native["HDRBLKS"]), "fields.HDRBLKS: decimal text")
        if codec == "brukerimage" and "header_bytes" in details:
            put("datastart", details["header_bytes"], "metadata.header.len", "derived_offset")
    elif codec == "HiPiCimage" and "shape" in details:
        put("Image_tag", bytes.fromhex(details["header_hex"])[:2].decode("ascii"), "header[0:2]")
        for key, value in zip(("Dim_1", "Dim_2"), reversed(details["shape"])):
            put(key, value, "metadata.shape", "alias")
        for key in ("Dim_1_offset", "Dim_2_offset"):
            if key in native:
                put(key, int(native[key]), "fields." + key + ": decimal text")
    elif codec == "fit2dspreadsheetimage" and "shape" in details:
        raw = bytes.fromhex(details["header_hex"])
        # FabIO calls the dimension line title (not the preceding title lines).
        for line in raw.splitlines(keepends=True):
            if b"Start pixel" in line:
                put("title", encode(line), "metadata.header: exact dimension line")
                break
        for key, value in zip(("Dim_1", "Dim_2"), reversed(details["shape"])):
            put(key, value, "metadata.shape", "alias")
    elif codec == "pnmimage":
        view = {}
        for key in ("SUBFORMAT", "WIDTH", "HEIGHT", "MAXVAL"):
            if key in native:
                put("b'" + key + "'", encode(native[key].encode("ascii")), "fields." + key + ": original token bytes")
    elif codec == "pixiimage" and "header_hex" in details:
        raw = bytes.fromhex(details["header_hex"])
        put("framesize", encode(np.frombuffer(raw[:4], dtype="<i4")), "metadata.headers[frame][0:4]")
        put("width", details["shape"][1], "metadata.shape", "alias")
        put("height", details["shape"][0], "metadata.shape", "alias")
        put("offset", len(raw), "metadata.headers[frame].len", "derived_offset")
    elif codec == "lambdaimage" and "detector" in native:
        put("detector", str(native["detector"].encode("utf-8")), "fields.detector: FabIO str(bytes) convention")
    elif codec == "speimage":
        for key in ("version", "data_type", "x_dim", "y_dim", "num_frames"):
            if key in native:
                put(key, int(native[key]), "fields." + key + ": decimal text")
        for key in ("exposure_time", "center_wavelength"):
            if key in native:
                put(key, encode(float(native[key])), "fields." + key + ": float64 text")
        if "x_calibration" in details:
            put("x_calibration", _array(details["x_calibration"], "<f8"), "metadata.x_calibration", "array_encoding")
        if details.get("regions"):
            put("roi", details["regions"][0]["roi"], "metadata.regions[0].roi", "alias")
    elif codec == "jpegimage" and details.get("jfif"):
        jfif = details["jfif"][-1]
        version = jfif["version"]
        put("jfif", version[0]*256 + version[1], "metadata.jfif_headers.last.version")
        put("jfif_version", version, "metadata.jfif_headers.last.version", "alias")
        put("jfif_unit", jfif["density_unit"], "metadata.jfif_headers.last.density_unit", "alias")
        put("jfif_density", jfif["density"], "metadata.jfif_headers.last.density", "alias")
    elif codec == "esperantoimage":
        if "version" in details:
            put("format", details["version"], "metadata.version", "alias")
        for key, value in native.items():
            put(key, " ".join(value.split()), "fields." + key + ": FabIO whitespace presentation")
        for key, value in details.get("parsed_fields", {}).items():
            put(key, value, "metadata.parsed_fields." + key, "alias")
    elif codec == "cbfimage":
        view = {}
        keys = {"content-type":"Content-Type", "content-transfer-encoding":"Content-Transfer-Encoding", "content-md5":"Content-MD5",
                "x-binary-id":"X-Binary-ID", "x-binary-size":"X-Binary-Size", "x-binary-element-type":"X-Binary-Element-Type",
                "x-binary-element-byte-order":"X-Binary-Element-Byte-Order", "x-binary-number-of-elements":"X-Binary-Number-of-Elements",
                "x-binary-size-fastest-dimension":"X-Binary-Size-Fastest-Dimension", "x-binary-size-second-dimension":"X-Binary-Size-Second-Dimension",
                "x-binary-size-padding":"X-Binary-Size-Padding"}
        for key, value in native.items():
            if key in keys:
                if key == "content-type":
                    parts = value.split(";", 1)
                    value = parts[0] + (";" if len(parts) == 2 else "")
                    if len(parts) == 2:
                        for item in parts[1].split(";"):
                            name, sep, text = item.partition("=")
                            if sep and name.strip().lower() == "conversions":
                                put("conversions", text.strip().strip('"'), "binary_fields.content-type: conversions parameter")
                elif key == "x-binary-element-type":
                    value = value.strip('"')
                put(keys[key], value, "binary_fields." + key)
        for key, value in details.get("cif_fields", {}).items():
            put(key, value.strip(' "\n\r\t') or "?", "metadata.cif.fields." + key + ": FabIO whitespace and empty-value presentation", "reference_presentation")
    elif codec == "fit2dimage":
        for key, value in details.get("array_values", {}).items():
            put(key, value, "metadata.arrays: read_fit2d_array_into", "array_encoding")
    elif codec == "mpaimage" and "header_hex" in details:
        # Compare the pinned flattened namespace while preserving ADC1/ADC2
        # independently in the native API. Repeated report titles overwrite
        # earlier reports in FabIO; native scoped values are never overwritten.
        view = {}
        scope = reference_scope = ""
        for line in bytes.fromhex(details["header_hex"]).decode("ascii").splitlines():
            line = line.strip()
            if "=" in line:
                key = line.split("=",1)[0].strip()
                source = scope+"_"+key if scope else key
                target = reference_scope+"_"+key if reference_scope else key
                if source in native:
                    put(target,native[source],"fields."+source,"reference_namespace")
            elif line.startswith("[DATA") or line.startswith("[CDAT"): break
            else:
                reference_scope = line.strip("[]")
                if line.startswith("["): scope = line[1:].split("]",1)[0]
    elif codec == "dm3image":
        view = {}
        def decoded(value):
            if "numeric" in value:
                array = dict(value["numeric"])
                if value["scalar"]: return array["values"][0]
                return array
            if "utf16" in value:
                return np.asarray(value["utf16"],dtype="<u2").tobytes().decode("utf-16-le")
            if "fields" in value:
                text = ""
                for item in value["fields"]:
                    part = item["value"]
                    if "numeric" not in part or not part["scalar"]: raise ValueError("unqualified DM3 reference struct field")
                    numeric = part["numeric"]
                    scalar = np.asarray(numeric["values"], dtype=numeric["dtype"])[0]
                    text += bytes.fromhex(item["name_hex"]).decode("utf-8")+" "+str(scalar)+" "
                return text
            if "opaque" in value: return None
            raise ValueError("unqualified DM3 value: "+str(value))
        for tag in details.get("tags", []):
            key = bytes.fromhex(tag["raw_name"]).decode("latin-1")
            descriptor = tag["descriptor"]
            value = tag["value"]
            if descriptor[:2] == [20,10]:
                put(key,None,"metadata.tags: reference omits unsigned-byte array","reference_presentation")
            elif value is not None:
                put(key,decoded(value),"metadata.tags: "+"/".join(tag["path"]),"reference_namespace")
            elif descriptor and descriptor[0] == 20 and len(descriptor) > 3:
                put(key,None,"metadata.tags: reference omits complex array","reference_presentation")
            elif key == "Data" and len(descriptor)==3 and descriptor[0]==20 and descriptor[-1] == math.prod(frame["shape"]):
                put(key,{"kind":"array","dtype":frame["dtype"],"shape":[descriptor[-1]],"sha256":frame["sample_sha256"]},"primary frame: exact flat stored Data array","array_encoding")
    elif codec == "mar345image":
        for key, value in details.get("binary_scalars", {}).items():
            if key == "NumPixels": value = str(value)
            put(key, value, "metadata.binary_scalars."+key, "documented_units")
        for key, value in details.get("text_scalars", []):
            put(key, value, "metadata.text_scalars: source-order assignment", "alias")
        if "DATE" in native and not native["DATE"].isascii():
            # The pinned reader truncates this corrupt DATE line to 39 source
            # bytes. The complete Latin-1 value remains native metadata.
            put("DATE", ("DATE".ljust(15)+native["DATE"])[:39].strip()[4:].strip(), "fields.DATE: pinned 39-byte corrupt-date presentation", "reference_presentation")
    elif codec == "OXDimage":
        aliases = {"Binning x":"Binning in x", "Binning y":"Binning in y", "Pixels x":"Pixels in x", "Pixels y":"Pixels in y",
                   "Number of pixels":"No of pixels", "Overflow flag":"Overflows flag", "Exposure time seconds":"Exposure time in sec",
                   "Overflow time seconds":"Overflow time in sec", "Monitor raw1":"Monitor counts of raw image 1", "Monitor raw2":"Monitor counts of raw image 2",
                   "Monitor overflow1":"Monitor counts of overflow raw image 1", "Monitor overflow2":"Monitor counts of overflow raw image 2",
                   "Pixel size x mm":"Real pixel size x (mm)", "Pixel size y mm":"Real pixel size y (mm)",
                   "Beam rotation e2 degrees":"Beam rot in deg (e2)", "Beam rotation e3 degrees":"Beam rot in deg (e3)",
                   "Distance mm":"Distance in mm", "Alpha angle degrees":"Alpha angle in deg", "Beta angle degrees":"Beta angle in deg",
                   "Stat minimum":"Stat: Min ", "Stat maximum":"Stat: Max ", "Stat average":"Stat: Average ", "Stat skewness":"Stat: Skewness "}
        aliases.update({f"Detector tilt e{i} degrees":f"Detector tilt e{i} in deg" for i in (1,2,3)})
        for source, key in aliases.items():
            if source in native: put(key, native[source], "fields."+source, "alias")
        for source, key in [("Spatial correction file","Spatial correction file"), ("Spatial correction date","Spatial correction file date"), ("Flood field image","Flood field image"), ("Time","Time")]:
            if source in native and native[source].get("hex") is not None:
                put(key, bytes.fromhex(native[source]["hex"]).strip(b"\0").decode("utf-8"), "fields."+source+": UTF-8 text")
        if "header_version" in details:
            for key, value, source in [("Header Version",details["header_version"],"header_version"),("Compression",bytes.fromhex(details["compression_label"]["hex"]).decode("ascii"),"compression"),
                ("NX",details["shape"][1],"shape"),("NY",details["shape"][0],"shape"),("OI",details["exception16"],"exception16"),("OL",details["exception32"],"exception32"),
                ("Header Size In Bytes",details["header_bytes"],"header.len"),("NSUPPLEMENT",0,"validated zero supplements")]:
                put(key,value,"metadata."+source,"alias")
            for section in details["sections"]:
                key = {"General":"General Section size in Byte", "Special":"Special Section size in Byte", "KM4":"KM4 Section size in Byte", "Statistics":"Statistic Section in Byte", "History":"History Section in Byte"}[section["name"]]
                put(key,section["bytes"],"metadata.sections."+section["name"],"alias")
            put("ASCII Section size in Byte",details["sections"][0]["offset"],"metadata.sections[0].offset","derived_offset")
        for axis in ("Omega","Theta","Kappa","Phi"):
            source = axis+" radians per motor step"
            if source in native:
                step = float(np.rad2deg(native[source]))
                put(axis+" step in deg",step,"fields."+source+": rad2deg","unit_conversion")
                for label, key in [("Start","start"),("End","end"),("Zero correction","zero corr.")]:
                    put(axis+" "+key+" in deg", step*native[axis+" "+label+" motor steps"], "fields: motor steps * degrees/step", "unit_conversion")
        if "Stat variance" in native:
            put("Stat: Stddev ",encode(float(np.sqrt(native["Stat variance"]))),"fields.Stat variance: sqrt","derived_statistic")
        types = {0:"Sapphire/KM4CCD (1x1: 0.06mm, 2x2: 0.12mm)",1:"Sapphire2-Kodak (1x1: 0.06mm, 2x2: 0.12mm)",2:"Sapphire3-Kodak (1x1: 0.03mm, 2x2: 0.06mm, 4x4: 0.12mm)",3:"Onyx-Kodak (1x1: 0.06mm, 2x2: 0.12mm, 4x4: 0.24mm)",4:"Unknown Oxford diffraction detector",7:"Pilatus 300K-Dectris"}
        if native.get("Detector type code") in types:
            put("Detector type", types[native["Detector type code"]], "fields.Detector type code: pinned label lookup")
    elif codec == "tifimage":
        tags = dict(native.get("tags", {}))
        for number, opaque in native.get("opaque_tags", {}).items():
            if opaque["field_type"] == 2 and number not in tags:
                tags[number] = bytes.fromhex(opaque["value_hex"]).split(b"\0",1)[0].decode("utf-8",errors="replace").replace("\ufffd", "?")
                mappings[{"270":"imageDescription"}.get(number, number)] = {"source":f"metadata.opaque_tags[{number}]: replacement-text display", "kind":"reference_presentation"}

        names = {256:"numberOfColumns",257:"numberOfRows",258:"bitsPerSample",259:"compression",262:"photometricInterpretation",
                 270:"imageDescription",272:"model",273:"stripOffsets",277:"samplesPerPixel",278:"rowsPerStrip",279:"stripByteCounts",
                 305:"software",306:"date",320:"colormap",339:"sampleFormat"}
        view = {}
        if backend == "PIL":
            for number, key in names.items():
                if str(number) in tags:
                    put(key, tags[str(number)], f"metadata.tags[{number}]", "alias")
        elif backend == "TiffIO" and tags:
            defaults = {"compression_type":1, "imageDescription":f"{frame['frame']+1}/{frame['frames']}",
                        "rowsPerStrip":native["shape"][0], "software":"Unknown Software", "date":"Unknown Date",
                        "sampleFormat":4,"photometricInterpretation":1}
            schema = {256:"nColumns",257:"nRows",258:"nBits",259:"compression_type",262:"photometricInterpretation",
                      270:"imageDescription",272:"model",273:"stripOffsets",278:"rowsPerStrip",279:"stripByteCounts",305:"software",306:"date",339:"sampleFormat"}
            for key, value in defaults.items():
                put(key, value, "pinned TiffIO absent-tag default", "synthetic_default")
            for number, key in schema.items():
                if str(number) in tags:
                    value = tags[str(number)]
                    if key in {"stripOffsets", "stripByteCounts", "model"} and not isinstance(value, list): value = [value]
                    put(key, value, f"metadata.tags[{number}]", "alias")
            if view["software"] == "Unknown Software" and str(view["imageDescription"]).upper().startswith("IMAGEJ"):
                put("software", view["imageDescription"].split("=")[0], "metadata.tags[270]: ImageJ software inference", "reference_presentation")
            if "stripByteCounts" not in view and "nBits" in view:
                bits = view["nBits"] if isinstance(view["nBits"], list) else [view["nBits"]]
                put("stripByteCounts", [sum(int(view["nRows"]*view["nColumns"]*b/8) for b in bits)], "TiffIO missing strip-count calculation from shape/bits", "synthetic_default")
            put("compression", view["compression_type"] != 1, "metadata.compression != 1")
            colormap = None
            if "320" in tags:
                values = np.asarray(tags["320"])
                if values.max() > 255: values = (values / 256.).astype(np.uint8)
                else: values = values.astype(np.uint8)
                colormap = encode(values.reshape(3,-1).T)
            put("colormap", colormap, "metadata.tags[320]: TiffIO 8-bit RGB palette", "reference_presentation")
            info = {}
            if view["software"].startswith("PyMca"):
                items = view["imageDescription"].split("=")
                info = {items[i*2]: items[i*2+1][:-1] for i in range(len(items)//2)}
            put("info", info, "metadata.tags[270,305]: TiffIO description dictionary", "reference_presentation")
    return view, mappings


def _equal(left, right):
    if type(left) is not type(right): return False
    if isinstance(left, dict):
        if left.get("kind") == right.get("kind") == "array":
            # Small values are a diagnostic preview; the canonical typed bytes,
            # dtype and complete shape are the array identity at every size.
            keys = ("kind","dtype","shape","sha256")
            return all(key in left and key in right and _equal(left[key],right[key]) for key in keys)
        return left.keys() == right.keys() and all(_equal(left[key], right[key]) for key in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(_equal(a,b) for a,b in zip(left,right))
    return left == right


def _digest(value):
    import json
    def identity(item):
        if isinstance(item,dict):
            keys = ("kind","dtype","shape","sha256") if item.get("kind")=="array" else item.keys()
            return {key:identity(item[key]) for key in keys}
        if isinstance(item,list): return [identity(value) for value in item]
        return item
    return hashlib.sha256(json.dumps(identity(value), sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def compare(reference, frame, codec, backend=None):
    view, mappings = project(codec, frame, backend)
    if codec == "edfimage":
        reference = {key.upper(): value for key, value in reference.items()}
        mappings.update({key:{"source":"fields."+key,"kind":"case_normalization"} for key in reference})
    if codec == "marccdimage":
        for key, value in reference.items():
            if isinstance(value, list) and value and all(isinstance(v,dict) and v.get("kind")=="bytes" and v["length"]==1 for v in value):
                if key in view:
                    view[key] = [encode(bytes([item])) for item in view[key]]
                    mappings[key] = {"source":"detector header byte array", "kind":"byte_array_representation"}
            elif isinstance(value, dict) and value.get("kind") == "bytes" and key in view:
                view[key] = encode(bytes(view[key]))
                mappings[key] = {"source":"detector header bytes", "kind":"byte_array_representation"}
    qualifications = reference_qualifications(codec, reference, frame, view)
    differences = []
    for key, expected in reference.items():
        if key not in view:
            differences.append({"field":key,"kind":"missing","reference":expected})
        elif not _equal(view[key], expected):
            differences.append({"field":key,"kind":"value_or_type","reference":expected,"native":view[key]})
    for difference in differences:
        if difference["field"] in qualifications:
            difference["qualification"] = qualifications[difference["field"]]
    unresolved = sum("qualification" not in item for item in differences)
    compared = {key:view[key] for key in reference if key in view}
    return {"status": "reference_empty" if not reference else "difference" if unresolved else "qualified_reference_difference" if differences else "match",
            "unresolved_fields":unresolved, "qualified_fields":len(differences)-unresolved,
            "reference_fields":len(reference), "reference_keys":sorted(reference), "matched_fields":len(reference)-len(differences),
            "native_fields":len(frame.get("fields", {})), "differences":differences,
            "field_mappings":{key:value for key,value in mappings.items() if key in reference},
            "reference_sha256":_digest(reference),"native_projection_sha256":_digest(compared)}


def summarize(reads):
    from collections import Counter
    result = {}
    for read in reads:
        row = result.setdefault(read["codec"], {"read_events":0, "compared_frames":0, "status":Counter(), "field_names":set(),
            "reference_fields":0,"matched_fields":0,"qualified_fields":0,"unresolved_fields":0,"differences":Counter(), "unresolved_differences":Counter(), "qualified_differences":Counter(), "mapping_kinds":Counter()})
        row["read_events"] += 1
        items = read.get("metadata") or [{"status":"not_compared"}]
        for item in items:
            row["status"][item["status"]] += 1
            row["field_names"].update(item.get("reference_keys",[]))
            row["compared_frames"] += "frame" in item
            row["reference_fields"] += item.get("reference_fields",0)
            row["matched_fields"] += item.get("matched_fields",0)
            row["qualified_fields"] += item.get("qualified_fields",0)
            row["unresolved_fields"] += item.get("unresolved_fields",0)
            for difference in item.get("differences",[]):
                row["qualified_differences" if "qualification" in difference else "unresolved_differences"][difference["field"]] += 1
            row["differences"].update(entry["field"] for entry in item.get("differences",[]))
            row["mapping_kinds"].update(entry["kind"] for entry in item.get("field_mappings",{}).values())
    for row in result.values(): row["field_names"] = sorted(row["field_names"])
    return result


def reference_qualifications(codec, reference, frame, view):
    """Evidence for pinned metadata behavior that should not replace source data.

    These differences remain differences and cannot establish metadata equality.
    Qualification requires exact reference-model AND native-source agreement;
    absent/incomplete evidence leaves an ordinary unresolved difference.
    """
    import struct
    details = frame.get("metadata_details", {})
    result = {}
    if codec == "fit2dimage" and "header_hex" in details:
        raw = bytes.fromhex(details["header_hex"])
        width = details["block_bytes"]
        constant = struct.unpack("<f", struct.pack("<I", 0x38d1b717))[0]
        for offset in range(0,len(raw),width):
            key, sep, record = raw[offset:offset+width].partition(b":")
            if sep and record[8:9] == b"r":
                key = key[1:].decode("ascii")
                bits = int(record[9:17],16)
                stored = struct.unpack("<f",struct.pack("<I",bits))[0]
                if key in reference and _equal(reference[key], constant) and _equal(view.get(key), encode(stored)):
                    result[key] = {"kind":"reference_float_decoder_defect", "source_bits_hex":f"{bits:08x}",
                        "source_record_offset":offset,"native_matches_stored_float":True,
                        "reference_matches_input_independent_constant":True,"constant_bits_hex":"38d1b717"}
    elif codec == "mrcimage" and "header_hex" in details:
        raw = bytes.fromhex(details["header_hex"])
        if len(raw) != 1024: return result
        order = "<" if details["little_endian"] else ">"
        keys = "NX NY NZ MODE NXSTART NYSTART NZSTART MX MY MZ CELL_A CELL_B CELL_C CELL_ALPHA CELL_BETA CELL_GAMMA MAPC MAPR MAPS DMIN DMAX DMEAN ISPG NSYMBT EXTRA ORIGIN MAP MACHST RMS NLABL".split()
        # Source-correct semantics, independent of the mrc backend used by Rust.
        actual = {}
        for index,key in enumerate(keys[:24]):
            fmt = "f" if 10 <= index <= 15 or 19 <= index <= 21 else "i"
            actual[key] = encode(struct.unpack_from(order+fmt,raw,index*4)[0])
        actual.update(EXTRA=encode(raw[96:196]), ORIGIN=list(struct.unpack_from(order+"3f",raw,196)),
            MAP=raw[208:212].decode("ascii"), MACHST=encode(raw[212:216]),
            RMS=encode(struct.unpack_from(order+"f",raw,216)[0]), NLABL=struct.unpack_from(order+"i",raw,220)[0])
        # FabIO ignores the selected MRC endian stamp and zips these semantic
        # names directly to 30 native-endian int32 words, skipping no EXTRA area.
        words = np.frombuffer(raw[:56*4],dtype=np.int32)
        for index,key in enumerate(keys):
            model = encode(words[index]) if key != "MAP" else words[index].tobytes().decode()
            if key in reference and _equal(reference[key],model) and _equal(view.get(key),actual[key]):
                result[key] = {"kind":"reference_mrc_word_layout_defect", "reference_word_index":index,
                    "reference_matches_raw_word_model":True,"native_matches_stored_semantic":True}
    elif codec == "GEimage" and details.get("kind") in {"ApsBlank8192","ApsEdf8192"}:
        # Explicit legacy compatibility policy, not real acquisition fields.
        # The native reader exposes the actual source kind and EDF fields.
        from fabio.GEimage import GE_HEADER_INFO
        raw = bytes.fromhex(details["header_hex"])
        position = 0
        model = {}
        for key,size,fmt in GE_HEADER_INFO:
            value = raw[position:position+size]
            model[key] = encode(struct.unpack(fmt,value)[0] if fmt is not None else value)
            position += size
        model.update(StandardHeaderSizeInBytes=8192,UserHeaderSizeInBytes=0,
            NumberOfRowsInFrame=details["shape"][0],NumberOfColsInFrame=details["shape"][1],ImageDepthInBits=16,NumberOfFrames=details["frames"])
        if details["kind"] == "ApsBlank8192" and any(raw): return result
        if details["kind"] == "ApsEdf8192" and int(details["edf_fields"].get("Num_Images",-1)) != details["frames"]: return result
        for key,value in reference.items():
            if key in model and _equal(value,model[key]):
                result[key] = {"kind":"reference_legacy_header_view", "source_kind":details["kind"],
                    "reference_matches_stored_bytes_and_explicit_geometry":True,
                    "acquisition_field_not_asserted_by_native":True}
    return result


def compact(reads):
    """Deduplicate reusable field mapping schemas; do not discard differences."""
    schemas = {}
    for read in reads:
        if "reference_header" in read:
            value = read.pop("reference_header")
            read["initial_reference_header"] = {"fields":len(value),"sha256":_digest(value)}
            if any(item["status"]=="not_compared" for item in read.get("metadata",[])):
                read["initial_reference_header"]["uncompared_values"] = value
        for item in read.get("metadata",[]):
            mappings = item.pop("field_mappings",None)
            if mappings is not None:
                schema = {"reference_keys":item.pop("reference_keys"),"field_mappings":mappings}
                key = _digest(schema)
                schemas[key] = schema
                item["mapping_schema_sha256"] = key
    return schemas
