#include <stdio.h>
#include <string.h>

struct kore_slice { void* ptr; long len; };

extern void run(int n, struct kore_slice items);

int main(void) {
    setbuf(stdout, NULL);
    static const char* words[] = {"alpha", "beta", "hello world"};
    struct kore_slice inner[3];
    for (int i = 0; i < 3; i++) {
        inner[i].ptr = (void*)words[i];
        inner[i].len = strlen(words[i]);
    }
    struct kore_slice outer = { inner, 3 };
    printf("-- calling run --\n");
    run(3, outer);
    printf("-- returned --\n");
    return 0;
}
