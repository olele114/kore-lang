#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct slice_u8 {
    char* ptr;
    long len;
};

struct slice_slice_u8 {
    struct slice_u8* ptr;
    long len;
};

void kore_main(int argc, struct slice_slice_u8 argv);

int main(int argc, char** argv) {
    struct slice_u8* slices = malloc(argc * sizeof(struct slice_u8));
    for (int i = 0; i < argc; i++) {
        slices[i].ptr = argv[i];
        slices[i].len = strlen(argv[i]);
    }
    
    struct slice_slice_u8 argv_slice;
    argv_slice.ptr = slices;
    argv_slice.len = argc;
    
    kore_main(argc, argv_slice);
    
    free(slices);
    return 0;
}
