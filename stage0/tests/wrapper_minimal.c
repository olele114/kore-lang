#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct kore_slice {
    void* ptr;
    long len;
};

// 声明外部符号，不使用 asm 重命名
extern void main(int, struct kore_slice);

// 使用不同的名字作为 C 入口点
int start_wrapper(int argc, char** argv) {
    printf("Starting C wrapper, argc=%d\n", argc);
    
    struct kore_slice* slices = malloc(argc * sizeof(struct kore_slice));
    for (int i = 0; i < argc; i++) {
        slices[i].ptr = argv[i];
        slices[i].len = strlen(argv[i]);
    }

    struct kore_slice argv_slice;
    argv_slice.ptr = slices;
    argv_slice.len = argc;
    
    printf("About to call Kore main...\n");
    main(argc, argv_slice);

    printf("Returned from Kore main\n");
    free(slices);
    return 0;
}
