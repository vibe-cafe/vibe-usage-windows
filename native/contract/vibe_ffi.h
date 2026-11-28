#ifndef VIBE_FFI_H
#define VIBE_FFI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define VIBE_ABI_VERSION 1u

/* x64, C calling convention. Inputs are borrowed NUL-terminated UTF-8.
 * The host must free every non-null output string with vibe_free. */
typedef void (*vibe_callback)(char *envelope);
typedef struct {
    uint64_t request_id;
    char *error;
} vibe_admission;

char *vibe_init(uint32_t version, const char *options_json, vibe_callback callback);
vibe_admission vibe_call(uint64_t request_id, const char *command, const char *args_json);
char *vibe_close(void);
char *vibe_free(char *value);

#ifdef __cplusplus
}
#endif
#endif
