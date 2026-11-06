/* The codec's original license is retained in tif_thunder.c and LICENSE.md. */
#define TIFFInitThunderScan dioptrin_unused_thunderscan_init
#include "tif_thunder.c"
int dioptrin_thunderscan_row(const uint8_t *input,size_t input_len,
                            uint32_t width,uint8_t *output,size_t output_len,
                            size_t *consumed) {
    if (!input || !output || !consumed || !width || input_len>PTRDIFF_MAX ||
        output_len<width/2+width%2) return -1;
#if PTRDIFF_MAX <= UINT32_MAX
    if (width>(uint32_t)(PTRDIFF_MAX-64)) return -1;
#endif
    TIFF tif={0};
    tif.tif_rawcp=(uint8_t *)input; tif.tif_rawcc=(tmsize_t)input_len;
    tif.tif_dir.td_bitspersample=4; tif.tif_dir.td_imagewidth=width;
    tif.tif_dir.td_scanlinesize=(tmsize_t)(width/2+width%2);
    int result=ThunderDecode(&tif,output,(tmsize_t)width);
    *consumed=input_len-(size_t)tif.tif_rawcc;
    return result==1 ? 1 : tif.dioptrin_error==2 ? -2 : 0;
}
