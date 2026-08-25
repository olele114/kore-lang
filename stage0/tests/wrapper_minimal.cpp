#include <cstdio>
#include <cstdlib>
#include <cstring>

struct kore_slice {
    void* ptr;
    long len;
};

extern "C" void kore_main(int, kore_slice);

extern "C" int main(int argc, char** argv) {
    printf("Starting C++ wrapper, argc=%d\n", argc);
    
    kore_slice* slices = (kore_slice*)malloc(argc * sizeof(kore_slice));
    for (int i = 0; i < argc; i++) {
        slices[i].ptr = argv[i];
        slices[i].len = strlen(argv[i]);
    }

    kore_slice argv_slice;
    argv_slice.ptr = slices;
    argv_slice.len = argc;
    
    printf("About to call Kore main...\n");
    kore_main(argc, argv_slice);

    printf("Returned from Kore main\n");
    free(slices);
    return 0;
}
