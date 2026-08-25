#include <stdio.h>

struct kore_slice {
    void* ptr;
    long len;
};

void test_fn(int a, struct kore_slice b) {
    printf("a=%d, b.ptr=%p, b.len=%ld\n", a, b.ptr, b.len);
}

int main() {
    struct kore_slice s;
    s.ptr = (void*)0x12345678;
    s.len = 42;
    test_fn(123, s);
    return 0;
}
