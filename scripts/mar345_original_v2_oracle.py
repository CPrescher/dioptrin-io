"""Test-only unchanged original JPA V2 writer/reader; no code vendored or linked."""
import hashlib,json,pathlib,struct,subprocess
from mar345_v2_reference import fingerprint,sha,parse,canonical,child_limits

SOURCE_URL="https://ftp.ccp4.ac.uk/opensource/libccp4-8.0.0.tar.gz"
ARCHIVE_SHA="cb813ae86612a0866329deab7cee96eac573d81be5b240341d40f9ad5322ff2d"
PINS={"pack_c.c":"90c61546fd44f6d56ca8d75806cfa4da86ff6f27aed911aeae56a2276dd7bcbb","pack_c.h":"a2ab66320c27637cc44599fd9f10630f31b44833d3a62b2e9a17246f4491dab6"}
WRAPPER=r'''
#include <stdio.h>
#include <stdint.h>
#include <limits.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <sys/resource.h>
#include "pack_c.h"
void v2unpack_word(FILE*,int,int,WORD*);
int main(int argc,char**argv){
 if(argc<6||sizeof(WORD)!=2||sizeof(LONG)!=4||CHAR_BIT!=8||SHRT_MIN!=-32768)return 2;
 struct rlimit mem={268435456,268435456};int applied=setrlimit(RLIMIT_AS,&mem);int memory_errno=applied?errno:0;
 int width=atoi(argv[4]),height=atoi(argv[5]);size_t count=(size_t)width*height,offset=strtoul(argv[3],NULL,10);
 if(width<2||height<2||width>8192||height>8192||count>32768)return 3;
 WORD*input=calloc(count+16,sizeof(WORD)),*output=calloc(count+16,sizeof(WORD));if(!input||!output)return 4;
 for(size_t i=count;i<count+16;i++)output[i]=(WORD)0x7bad;
 FILE*f=fopen(argv[2],"rb");if(!f)return 5;fseek(f,0,SEEK_END);long n=ftell(f);if(n<0||n>1048576||offset>(size_t)n)return 6;rewind(f);
 int roundtrip=strcmp(argv[1],"roundtrip")==0;
 if(roundtrip){
  uint16_t little=1;if(argc!=7||*(unsigned char*)&little!=1||n!=(long)(count*2))return 7;
  if(fread(input,2,count,f)!=count)return 8;fclose(f);
  v2pack_wordimage_c(input,width,height,argv[6]);readpack_word_c(output,argv[6]);
 }else{if(strcmp(argv[1],"decode")!=0)return 9;fseek(f,(long)offset,SEEK_SET);v2unpack_word(f,width,height,output);fclose(f);}
 for(size_t i=count;i<count+16;i++)if(output[i]!=(WORD)0x7bad)return 10;
 printf("{\"status\":\"decoded\",\"abi\":{\"word_bytes\":%zu,\"long_bytes\":%zu,\"char_bits\":%d},\"memory_limit\":{\"attempted_bytes\":268435456,\"applied\":%s,\"errno\":%d},\"output_guard_intact\":true,\"roundtrip_equal\":",sizeof(WORD),sizeof(LONG),CHAR_BIT,applied?"false":"true",memory_errno);
 if(roundtrip)printf("%s",memcmp(input,output,count*2)==0?"true":"false");else printf("null");
 printf(",\"values\":[");for(size_t i=0;i<count;i++){if(i)putchar(',');printf("%u",(unsigned short)output[i]);}puts("]}");free(input);free(output);return 0;
}
'''


def build(source,directory):
    files=[source/name for name in PINS]
    for file in files:
        if sha(file.read_bytes())!=PINS[file.name]:raise ValueError("original CCP4 source/header differs from pinned official archive")
    archive=source/"libccp4-8.0.0.tar.gz"
    if sha(archive.read_bytes())!=ARCHIVE_SHA:raise ValueError("original CCP4 official archive differs from pin")
    # Verify the actual files against the official archive, not only independent hashes.
    import tarfile
    with tarfile.open(archive,"r:gz") as handle:
        for file in files:
            matches=[m for m in handle.getmembers() if m.isfile() and m.name.endswith('/ccp4/'+file.name)]
            if len(matches)!=1 or handle.extractfile(matches[0]).read()!=file.read_bytes():raise ValueError("original CCP4 file/archive identity differs")
    wrapper=directory/"original.c";wrapper.write_text(WRAPPER);binary=directory/"original"
    command=["cc","-std=c99","-O0","-I",str(source),str(wrapper),str(source/"pack_c.c"),"-o",str(binary)]
    result=subprocess.run(command,capture_output=True,text=True,timeout=30,check=True)
    return binary,{"origin":{"url":SOURCE_URL,"archive":fingerprint(archive)},"sources":[fingerprint(p) for p in files],"wrapper":fingerprint(wrapper),"binary":fingerprint(binary),"command":command,"compiler":subprocess.check_output(["cc","--version"],text=True),"stderr":result.stderr,"license":"Original JPA1995 CCP4 modified LGPL3/UK-law notice retained in untouched external source; test-only compiler/executable, no native incorporation or expression translation","contract":"Original v2pack_wordimage_c emits V2IDENTIFIER, calls signed WORD diff_words then7-bit3+4 v2pack_chunk; original v2unpack_word consumes7-bit alphabet[0,3..16,32] and signed WORD predictor","bounds":"fresh exact source/table/block/tail admission; <=1MiB source, <=32768 words, signed-short/32-bit-LONG ABI checked; CPU8s/wall20s/fileoutput2MiB/core0, AS256MiB actual outcome recorded;16 output guard words"}


def invoke(binary,args,count):
    result=subprocess.run([str(binary),*map(str,args)],capture_output=True,text=True,timeout=20,preexec_fn=child_limits)
    if result.returncode:raise ValueError(f"original isolated oracle failed:{result.returncode}:{result.stderr}")
    output=json.loads(result.stdout);values=output.pop("values")
    if not output["output_guard_intact"] or len(values)!=count:raise ValueError("original output guard/count mismatch")
    return output,values


def decode(binary,path,record):
    if parse(path.read_bytes(),"original")!=record:raise ValueError("original source changed after admission")
    height,width=record["metadata"]["shape"]
    result,values=invoke(binary,["decode",path,record["offset"],width,height],width*height)
    for high in record["metadata"]["overflow_records"]:values[high["one_based_index"]-1]=high["value"]
    result.update(shape=[height,width],dtype="uint32",sample_sha256=sha(canonical(values)))
    return result,values


def authored_cases():
    signed=[32768+i for i in range(9)]+[65535,32768,1,0,65534,17,32767,65535,0,1,65535,32768,32769,2,3]
    varied=[0,65535,32768,32767,1,65534,17,0]*4
    words=[];state=0x12345678
    for _ in range(128):state=(1664525*state+1013904223)&0xffffffff;words.append(state&65535)
    return [("signed-neighborhood",[4,6],signed),("unsigned-first-row",[4,8],list(range(32768,32800))),("reserved-words",[4,8],varied),("positive",[4,8],[i%5 for i in range(32)]),("authored-varied",[8,16],words)]


def roundtrip(binary,directory,name,shape,words,header):
    if len(shape)!=2 or any(type(n) is not int or not 2<=n<=8192 for n in shape) or not 4<=len(words)<=32768 or len(words)!=shape[0]*shape[1] or any(type(n) is not int or not 0<=n<=65535 for n in words):raise ValueError("original encoder input admission")
    path=directory/(name+'.mar345');raw=directory/(name+'.u16');raw.write_bytes(struct.pack('<'+str(len(words))+'H',*words))
    if len(header)!=4096:raise ValueError("original encoder header admission")
    path.write_bytes(header)
    result,output=invoke(binary,["roundtrip",raw,0,shape[1],shape[0],path],len(words))
    if not result["roundtrip_equal"] or output!=words:raise ValueError("unchanged original encoder/decoder fails authored U16 roundtrip")
    record=parse(path.read_bytes(),"original")
    result.update(shape=shape,source_words_sha256=sha(raw.read_bytes()),sample_sha256=sha(canonical(words)),source_file=fingerprint(path),encoded_sha256=sha(record["encoded"]),marker_hex=record["metadata"]["marker_hex"])
    return path,record,result
