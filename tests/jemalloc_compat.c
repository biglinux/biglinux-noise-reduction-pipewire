/* SPDX-License-Identifier: MIT
 * Native dependency contract: free_sized(NULL, n) must do nothing, including
 * when n is nonzero. GLib legitimately uses this C23 interface.
 * Compile with -Wl,--no-as-needed -ljemalloc and run under a timeout.
 */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern int mallctl(const char *, void *, size_t *, void *, size_t);
extern void free_sized(void *, size_t);
extern void free_aligned_sized(void *, size_t, size_t);

int main(void) {
    const char *version = NULL;
    size_t version_size = sizeof(version);
    assert(mallctl("version", &version, &version_size, NULL, 0) == 0);
    printf("Testing jemalloc %s C23 deallocation contract\n", version);
    fflush(stdout);

    void (*volatile sized)(void *, size_t) = free_sized;
    void (*volatile aligned_sized)(void *, size_t, size_t) = free_aligned_sized;
    const size_t sizes[] = {0, 1, 8, 16, 64, 256, 4096, SIZE_MAX};
    for (size_t iteration = 0; iteration < 128; ++iteration) {
        for (size_t index = 0; index < sizeof(sizes) / sizeof(sizes[0]); ++index) {
            sized(NULL, sizes[index]);
            aligned_sized(NULL, 64, sizes[index]);
            unsigned char *memory = calloc(64, 1);
            assert(memory != NULL);
            for (size_t byte = 0; byte < 64; ++byte) {
                assert(memory[byte] == 0);
            }
            memset(memory, 0xa5, 64);
            sized(memory, 64);

            memory = aligned_alloc(64, 128);
            assert(memory != NULL);
            assert((uintptr_t)memory % 64 == 0);
            memset(memory, 0x5a, 128);
            aligned_sized(memory, 64, 128);
        }
    }
    puts("C23 sized deallocation: PASS");
    return EXIT_SUCCESS;
}
