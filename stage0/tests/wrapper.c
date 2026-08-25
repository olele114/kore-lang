#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct kore_slice {
    void* ptr;
    long len;
};

void kore_main_impl(int argc, struct kore_slice argv) asm("main");

int main(int argc, char** argv) {
    printf("C wrapper: argc=%d\n", argc);
    
    struct kore_slice* slices = malloc(argc * sizeof(struct kore_slice));
    for (int i = 0; i < argc; i++) {
        slices[i].ptr = argv[i];
        slices[i].len = strlen(argv[i]);
        printf("  slices[%d]: ptr=%p len=%ld str='%s'\n", 
               i, slices[i].ptr, slices[i].len, (char*)slices[i].ptr);
    }

    struct kore_slice argv_slice;
    argv_slice.ptr = slices;
    argv_slice.len = argc;
    
    printf("Calling Kore main with argv_slice: ptr=%p len=%ld\n", 
           argv_slice.ptr, argv_slice.len);

    kore_main_impl(argc, argv_slice);

    free(slices);
    return 0;
}
