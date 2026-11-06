/* Local row-only ABI for the unchanged permissive libtiff ThunderScan codec.
 * This does not parse TIFF, allocate native image buffers or access source files. */
#ifndef DIOPTRIN_THUNDER_SHIM_H
#define DIOPTRIN_THUNDER_SHIM_H
#include <stdint.h>
#include <stddef.h>
#include <inttypes.h>
#include <stdarg.h>
#include <string.h>
#define THUNDER_SUPPORT 1
typedef ptrdiff_t tmsize_t;
typedef struct TIFF TIFF;
struct TIFF {
    int dioptrin_error;
    uint8_t *tif_rawcp;
    tmsize_t tif_rawcc;
    struct { uint16_t td_bitspersample; uint32_t td_imagewidth; unsigned long td_row; tmsize_t td_scanlinesize; } tif_dir;
    int (*tif_setupdecode)(TIFF *);
    int (*tif_decoderow)(TIFF *,uint8_t *,tmsize_t,uint16_t);
    int (*tif_decodestrip)(TIFF *,uint8_t *,tmsize_t,uint16_t);
};
static void TIFFErrorExtR(TIFF *tif,const char *module,const char *format,...) {
    (void)format;
    tif->dioptrin_error=3;
    if (strcmp(module,"ThunderDecode")==0) {
        va_list args; va_start(args,format);
        const char *kind=va_arg(args,const char *);
        tif->dioptrin_error=strcmp(kind,"Too much")==0 ? 2 : 1;
        va_end(args);
    }
}
#endif
