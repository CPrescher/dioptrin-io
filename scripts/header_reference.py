"""Independent TIFF tag comparison through tifffile; never calls page.asarray."""
import pathlib
import numpy as np
import tifffile
from metadata_parity import encode, _equal


def tag_value(tag,raw,order):
    import struct
    big=tag.parent.is_bigtiff
    raw.seek(tag.offset)
    entry=raw.read(20 if big else 12)
    kind=int(tag.dtype);count=tag.count
    widths={1:1,2:1,3:2,4:4,5:8,6:1,7:1,8:2,9:4,10:8,11:4,12:8,13:4,16:8,17:8,18:8}
    value_position=12 if big else 8
    payload_position=tag.offset+value_position if count*widths[kind]<=(8 if big else 4) else struct.unpack(order+('Q' if big else 'I'),entry[value_position:])[0]
    raw.seek(payload_position)
    if kind==2:
        data=raw.read(count)
        if count<= (8 if tag.parent.is_bigtiff else 4):return data.decode("ascii").strip("\0")
        return data.split(b"\0",1)[0].decode("utf-8")
    codes={1:'B',3:'H',4:'I',5:'I',6:'b',7:'B',8:'h',9:'i',10:'i',11:'f',12:'d',13:'I',16:'Q',17:'q',18:'Q'}
    code=codes[kind];n=count*(2 if kind in (5,10) else 1)
    values=list(struct.unpack(order+str(n)+code,raw.read(struct.calcsize(code)*n)))
    if kind in (5,10):
        values=[{'kind':'rational','numerator':values[i],'denominator':values[i+1]} for i in range(0,len(values),2)]
    return encode(values[0] if count==1 else values)


def compare(path,frames):
    differences=[];first_bits=None
    with tifffile.TiffFile(path) as source, pathlib.Path(path).open('rb') as raw:
        if len(source.pages)!=len(frames):differences.append({'field':'page_count','reference':len(source.pages),'native':len(frames)})
        for index,(page,frame) in enumerate(zip(source.pages,frames)):
            raw.seek(0)
            if frame.get('source_prefix_hex')!=raw.read(64).hex():differences.append({'frame':index,'field':'source_prefix'})
            if frame.get('pixels_available') is not False:differences.append({'frame':index,'field':'header_only_must_not_claim_pixels'})
            tags=frame['fields']['tags'];opaque=frame.get('opaque_tags',{})
            if index==0 and 258 in page.tags:first_bits=tag_value(page.tags[258],raw,source.byteorder)
            if frame['ifd_offset']!=page.offset:differences.append({'frame':index,'field':'ifd_offset'})
            for number,tag in page.tags.items():
                key=str(number)
                if key in opaque:
                    raw.seek(tag.valueoffset);payload=raw.read(tag.count*{1:1,2:1,3:2,4:4,5:8,6:1,7:1,8:2,9:4,10:8,11:4,12:8,13:4,16:8,17:8,18:8}[int(tag.dtype)])
                    expected={'field_type':int(tag.dtype),'count':tag.count,'value_hex':payload.hex()}
                    if not _equal(expected,opaque[key]):differences.append({'frame':index,'field':key,'reference':expected,'native':opaque[key]})
                else:
                    expected=tag_value(tag,raw,source.byteorder)
                    if key not in tags or not _equal(expected,tags[key]):differences.append({'frame':index,'field':key,'reference':expected,'native':tags.get(key)})
            extras=(set(tags)|set(opaque))-{str(k) for k in page.tags.keys()}
            if extras:differences.append({'frame':index,'field':'unexpected_tags','native':sorted(extras)})
    return {'status':'match' if not differences else 'difference','pages':len(frames),'tag_occurrences':sum(len(f['fields']['tags'])+len(f.get('opaque_tags',{})) for f in frames),'differences':differences,'first_bits':first_bits,'oracle':'tifffile metadata only; no pixel decode'}
