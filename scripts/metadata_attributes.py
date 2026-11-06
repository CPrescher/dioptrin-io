"""Inventory metadata outside FabIO .header, separately from decoder state.

Unknown populated public attributes remain unclassified. No repr of a live
file/decoder object is used as source metadata or parity evidence.
"""
import numpy as np
from metadata_parity import encode

SOURCE = {
    'numpyimage': {'dataset'},
    'cbfimage': {'cif','pilatus_headers'},
    'brukerimage': {'version'}, 'bruker100image': {'version'},
    'edfimage': {'generalframe','incomplete_data','incomplete_file'},
    'dm3image': {'byte_order','bytes_in_file'},
    'eigerimage': {'dataset'}, 'hdf5image': {'dataset'}, 'lambdaimage': {'dataset'}, 'limaimage': {'dataset'},
    'esperantoimage': {'HEADER_LINES','format'}, 'fit2dimage': {'BUFFER_SIZE'},
    'tifimage': {'nbits','tiff_header'}, 'jpegimage': {'nbits'}, 'jpeg2kimage': {'nbits'},
    'pilatusimage': {'nbits'}, 'marccdimage': {'nbits'},
    'sparseimage': {'background_avg','background_std','cutoff','dataset','dummy','frame_ptr','index','intensity','mask','noisy','normalization','peaks','radius'},
    'mar345image': {'byte_order','byteorder','numhigh','numpixels'},
    'mrcimage': {'imagesize'}, 'pilatusimage': {'nbits','pilatus_header'},
    'speimage': {'num_frames','xml_offset'}, 'raxisimage': {'endianness'}, 'xsdimage': {'coding','md5','size'},
}
STATE = {'filenumber','mean','stddev','maxval','minval','roi','slice','area_sum','filename','currentframe','sequencefilename','data','header','h5','hdf5','file','infile','lib',
         'cbf','cbs','start_binary','bfname','bfsize','bfstart','blobsize','filesize','size','start',
         'data_type','encoded_datatype','grouptag_is_open','grouptag_is_sorted','grouptag_no_tags','no_data_elements',
         'tag_data_type','tag_encoded_type','tag_is_data','tag_label_length','num_block','blob','lstChr','raw16','raw32','rest','rawData'}


def value(item):
    import enum
    if isinstance(item,enum.Enum):return value(item.value)
    if isinstance(item,np.dtype):return {"kind":"dtype","value":item.name}
    if isinstance(item, (np.ndarray,np.generic,str,bytes,int,float,bool,list,tuple,dict,type(None))):
        if isinstance(item,(list,tuple)):return [value(x) for x in item]
        if isinstance(item,dict):return {str(k):value(v) for k,v in item.items()}
        return encode(item)
    if hasattr(item,'_dict'):return encode(dict(item._dict))
    if hasattr(item,'header'):return {'kind':'frame_metadata','header':encode(dict(item.header))}
    # HDF5 datasets/groups are described without loading their pixel payloads.
    if hasattr(item,'name') and hasattr(item,'file'):
        import pathlib
        result={'kind':'hdf5_object','path':item.name,'source_file':str(pathlib.Path(item.file.filename).resolve())}
        if hasattr(item,'shape'):result.update(shape=list(item.shape),dtype=item.dtype.name)
        return result
    return {'kind':'unhandled','python_type':type(item).__qualname__}


def inventory(codec,image):
    sources = SOURCE.get(codec,set()) | {"shape","dtype","nframes"}
    items = {}
    for name,item in vars(image).items():
        if name.startswith('_'):continue
        category='source_metadata' if name in sources else 'reader_state' if name in STATE else 'unclassified'
        items[name]={'category':category,'python_type':type(item).__qualname__}
        if category!='reader_state':
            try:items[name]['value']=value(item)
            except Exception as error:items[name]['error']=f'{type(error).__name__}: {error}'
    # Source-defined properties/class defaults and common geometry properties.
    for name in sources-items.keys():
        try:
            item=getattr(image,name)
            items[name]={'category':'source_metadata','value':value(np.dtype(item) if name=='dtype' and item is not None else item)}
        except AttributeError as error:
            if name in {'shape','dtype'} and image.data is None:
                items[name]={'category':'source_metadata','reference_undefined':True,'error':type(error).__name__}
        except Exception as error:
            items[name]={'category':'source_metadata','error':f'{type(error).__name__}: {error}'}
    return items


def compare_attributes(read,frames,prefix=None,source_bits=None):
    import pathlib
    import math
    from metadata_parity import project,_equal,_digest
    codec=read['codec'];items=read.get('reference_attributes',{})
    first=frames[0] if frames else {};details=first.get('metadata_details',{});native={};mapping={}
    def put(key,item,source):native[key]=item;mapping[key]=source
    if frames:
        selected=first.get('reference_view',first)
        if first.get('source_recovery',{}).get('format')=='edf':
            image=first['source_recovery']['images'][0]
            selected={'shape':image['availability']['declared_shape'],'dtype':image['dtype']}
            frame_count=len(first['source_recovery']['images'])
        else:frame_count=first.get('frames')
        if first.get('source_recovery',{}).get('format')=='tiff-thunderscan':selected={**first,'shape':first['source_recovery']['declared_shape']}
        proofs=read.get('reference_defect_checks',[])
        if read['status']=='reference_decoder_defect' and proofs and all(p['proof'].get('qualified') for p in proofs):
            selected=proofs[0]['proof']['corrected_luminance']
        if read['status']=='reference_no_pixels':
            if codec=='edfimage' and first.get('header_only') is True and first.get('pixels_available') is False:
                if 'declared_shape' in first:
                    put('shape',first['declared_shape'],'native EDF header declared_shape; metadata only, no decoded samples')
                if 'declared_dtype' in first:
                    put('dtype',{'kind':'dtype','value':first['declared_dtype']},'native EDF header declared_dtype; metadata only, no decoded samples')
            elif codec=='dtrekimage':put('shape',first.get('declared_shape'),'native declared source array shape')
            elif codec=='tifimage':
                tags=first['fields']['tags'];put('shape',[tags.get('257'),tags.get('256')],'native original TIFF rows/columns')
            put('nframes',len(frames),'native primary image/header count')
        else:
            put('shape',details.get('source_shape',selected.get('shape')) if codec=='dtrekimage' else selected.get('shape'),'native logical source array or explicit selected image view')
            put('dtype',{'kind':'dtype','value':selected.get('dtype')},'native logical scalar dtype of selected image view; storage byte order retained separately')
            put('nframes',frame_count,'native indexed image count')
    if codec=='numpyimage' and frames:
        source_shape=details.get('source_shape',[])
        shape=[1,math.prod(source_shape)] if len(source_shape)<2 else [math.prod(source_shape[:-2]),*source_shape[-2:]] if len(source_shape)>3 else source_shape
        aggregate=frames[-1].get('metadata_details',{}).get('dataset_sha256')
        if aggregate is not None:
            put('dataset',{'kind':'array','shape':shape,'dtype':first['dtype'],'sha256':aggregate},'native complete logical array bytes; explicit reference axis reshape, original axes retained')
    if codec=='cbfimage':
        cif,_=project(codec,first)
        cif={key:value for key,value in cif.items() if key.startswith('_')}
        cif['_array_data.data']='CIF Binary Section'
        if details.get('cif_loops'):
            cif['loop_']=[ [group['columns'],[{key:value for key,value in zip(group['columns'],row)} for row in group['rows']]] for group in details['cif_loops']]
        put('cif',cif,'metadata.cif; binary section placeholder is pinned reference presentation')
        put('pilatus_headers',details.get('pilatus'),'metadata.pilatus.typed_fields')
    if codec.startswith('bruker'):put('version',details.get('version'),'metadata.version')
    if codec=='dm3image':
        put('byte_order','<' if details.get('little_endian') else '>','metadata.little_endian')
        put('bytes_in_file',details.get('declared_bytes'),'metadata.declared_bytes')
    if codec=='esperantoimage':
        put('HEADER_LINES',details.get('header_lines'),'metadata.header_lines')
        put('format',{'AgiBitfield':'AGI_BITFIELD','Long32':'4BYTE_LONG'}.get(details.get('encoding')),'metadata.encoding')
    if codec=='fit2dimage':put('BUFFER_SIZE',details.get('block_bytes'),'metadata.block_bytes')
    if codec=='mar345image':
        words=details.get('binary_scalars',{})
        put('numhigh',words.get('NumHigh'),'metadata.binary_words[2]')
        put('numpixels',words.get('NumPixels'),'metadata.binary_words[5]')
        for key in ('byteorder','byte_order'):put(key,'<' if details.get('little_endian') else '>','metadata.little_endian')
    if codec=='mrcimage' and frames:put('imagesize',math.prod(first['shape'])*np.dtype(first['dtype']).itemsize,'native frame geometry and source dtype')
    if codec=='speimage':
        put('xml_offset',details.get('xml_offset'),'metadata.header source offset 678')
        put('num_frames',details.get('frames'),'metadata.frames')
    if codec=='raxisimage':put('endianness','<' if details.get('little_endian') else '>','metadata.little_endian')
    if codec=='xsdimage':
        for key,source in [('coding','coding'),('md5','md5'),('size','elements')]:put(key,details.get(source),'metadata.'+source)
    if codec in {'hdf5image','eigerimage','limaimage','lambdaimage'}:
        datasets=[];seen=set()
        for frame in frames:
            info=frame.get('metadata_details',{})
            if 'dataset' not in info:continue
            source_file=str(pathlib.Path(info['source_file']).resolve())
            key=(source_file,info['dataset'])
            if key not in seen:
                seen.add(key);datasets.append({'kind':'hdf5_object','path':info['dataset'],'source_file':source_file,'shape':info['source_shape'],'dtype':frame['dtype']})
        put('dataset',datasets if codec=='eigerimage' else datasets[0] if datasets else None,'frozen native HDF5 dataset descriptors and tracked source identities')
    if codec=='edfimage':
        if first.get('source_recovery'):
            report=first['source_recovery'];availability=report['images'][0]['availability'];incomplete=availability['present_samples'][1]-availability['present_samples'][0]!=availability['declared_samples']
            general=report.get('general_headers',[])
        else:
            incomplete=details.get('incomplete_data',False);general=details.get('general_headers',[])
        whole_incomplete=first.get('source_recovery',{}).get('stop',{}).get('kind') not in (None,'EndOfSource','StoragePadding') or any(image['availability']['present_samples'][1]-image['availability']['present_samples'][0]!=image['availability']['declared_samples'] for image in first.get('source_recovery',{}).get('images',[]))
        put('incomplete_file',whole_incomplete,'native source stop state and every available sample span')
        put('incomplete_data',incomplete,'native complete-source or recovery availability/integrity report')
        if general:
            header=general[-1].get('fields',general[-1])
            put('generalframe',{'kind':'frame_metadata','header':header},'native general_headers source-order metadata')
        else:put('generalframe',None,'native general_headers absence')
    if codec=='sparseimage':
        components=first.get('source_components',[])
        byname={c['name']:c for c in components}
        for name in ('mask','radius','background_avg','background_std','frame_ptr','index','intensity','normalization'):
            c=byname.get(name)
            put(name,None if c is None else {'kind':'array','dtype':c['dtype'],'shape':c['shape'],'sha256':c['sample_sha256']},'native sparse component '+name)
        put('dummy',byname['dummy']['scalar_value'] if 'dummy' in byname else {'kind':'float','value':'nan'} if byname.get('intensity',{}).get('dtype','').startswith('float') else 0,'source dummy component or explicit default from stored intensity dtype')
        put('noisy',0.0,'explicit deterministic reconstruction policy NOISY=False')
        cutoff=None
        import json
        for component in details.get('component_inventory',[]):
            if component['name'].startswith('entry/sparsify/') and component.get('text'):
                for text in component['text']:
                    try:configuration=json.loads(text)
                    except (ValueError,TypeError):continue
                    if isinstance(configuration,dict):cutoff=configuration.get('sparsify',{}).get('cutoff_pick')
        put('cutoff',cutoff,'native original sparsify configuration text and explicit absent-key default')
        peaks=next((path for path in details.get('group_inventory',[]) if path.endswith('/peaks')),None)
        put('peaks',peaks,'native original group inventory')
    comparisons=[]
    for name,item in items.items():
        category=item['category']
        if category=='reader_state':continue
        expected=item.get('value')
        result={'attribute':name,'category':category,'reference':expected}
        if item.get('reference_undefined') and read['status']=='reference_no_pixels':
            result.update(status='reference_unspecified',reason='reference property is undefined without pixels')
        elif name=='shape' and codec=='tifimage' and read['status']=='reference_no_pixels' and not _equal(expected,native.get(name)):
            raw=bytes.fromhex(first.get('source_prefix_hex',''))
            model=[int(n) for n in np.frombuffer(raw[:64],dtype=np.uint16)[[15,9]]] if len(raw)>=64 else None
            if _equal(expected,model) and read.get('header_only',{}).get('status')=='match':
                result.update(status='qualified_reference_difference',native=native[name],source=mapping[name],qualification='pinned preliminary TIFF shape words are not the independently verified stored geometry')
            else:result.update(status='difference',native=native.get(name))
        elif name=='incomplete_file' and codec=='edfimage' and expected is True and native.get(name) is False and read['status']=='counts_match':
            tail=bytes.fromhex(read.get('metadata_integrity_evidence',{}).get('tail_hex',''))
            if 0<len(tail)<512 and all(byte in b"\0 \r\n\t\v\f" for byte in tail):
                result.update(status='qualified_reference_difference',native=False,source=mapping[name],qualification='reference treats independently verified bounded storage padding as incomplete; every native frame matches')
            else:result.update(status='difference',native=False)
        elif name=='nframes' and codec=='speimage' and type(expected) is int and expected==1 and native.get(name)==len(frames) and native.get(name)!=1 and details.get('frames')==len(frames) and int(first.get('fields',{}).get('num_frames',-1))==len(frames):
            result.update(status='qualified_reference_difference',native=native[name],source=mapping[name],qualification='pinned SPE inherited nframes remains 1; every declared native frame was independently compared')
        elif name=='nframes' and codec=='dtrekimage' and type(expected) is int and expected==1 and native.get(name)==len(frames) and native.get(name)!=1 and details.get('source_shape') and len(details['source_shape'])>2 and math.prod(details['source_shape'][:-2])==len(frames):
            result.update(status='qualified_reference_difference',native=native[name],source=mapping[name],qualification='reference exposes one multidimensional array; native exposes its source-ordered 2D planes with full axes retained')
        elif 'error' in item or category=='unclassified':result.update(status='unclassified',error=item.get('error'))
        elif name=='nbits' and expected is None:result.update(status='reference_unspecified',reason='reference placeholder has no source value')
        elif name=='nbits' and codec in {'tifimage','pilatusimage'}:
            # FabIO labels native-endian word 21 from the first 64 source bytes
            # as nbits. It is unrelated to the TIFF BitsPerSample tag.
            raw=bytes.fromhex(prefix or first.get('source_prefix_hex',''))
            tags=details.get('image',first.get('fields',{})).get('tags',{})
            if type(expected) is int and len(raw)>=44 and int(np.frombuffer(raw[:64],dtype=np.uint16)[21])==expected and source_bits is not None and _equal(tags.get('258'),source_bits):
                result.update(status='qualified_reference_difference',native=tags['258'],source='native original TIFF tag 258',qualification='pinned preliminary-header word probe is not source bit depth')
            else:result.update(status='difference',reason='unqualified preliminary-header probe')
        elif name not in native:result.update(status='missing')
        else:
            actual=native[name]
            if name=='generalframe' and expected is not None and actual is not None:
                expected={'kind':'frame_metadata','header':{k.upper():v for k,v in expected['header'].items()}}
            result.update(status='match' if _equal(expected,actual) else 'difference',native=actual,source=mapping[name])
        comparisons.append(result)
    # Matching big arrays retain canonical identities without redundant previews.
    for result in comparisons:
        if result['status']=='match':
            result['reference_sha256']=_digest(result.pop('reference'))
            result['native_sha256']=_digest(result.pop('native'))
    bad=[c for c in comparisons if c['status'] not in {'match','covered_by_pixel_comparison','reference_unspecified','qualified_reference_difference'}]
    return {'status':'difference' if bad else 'accounted','compared_attributes':len(comparisons),'unresolved_attributes':len(bad),'comparisons':comparisons}


def summarize_attributes(reads):
    from collections import Counter
    summary={}
    for read in reads:
        row=summary.setdefault(read['codec'],{'read_events':0,'source_attributes':set(),'reader_state':set(),'statuses':Counter(),'unresolved':Counter()})
        row['read_events']+=1
        for name,item in read.get('reference_attributes',{}).items():
            row['reader_state' if item['category']=='reader_state' else 'source_attributes'].add(name)
        comparison=read.get('attribute_comparison',{})
        for item in comparison.get('comparisons',[]):
            row['statuses'][item['status']]+=1
            if item['status'] not in {'match','covered_by_pixel_comparison','reference_unspecified','qualified_reference_difference'}:row['unresolved'][item['attribute']]+=1
        if comparison.get('status')=='comparison_error':row['unresolved']['comparison_error']+=1
    for row in summary.values():
        for key in ('source_attributes','reader_state'):row[key]=sorted(row[key])
    return summary
