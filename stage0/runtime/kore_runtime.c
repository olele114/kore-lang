#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// 全局变量存储命令行参数
static int g_argc = 0;
static char **g_argv = NULL;

// 初始化命令行参数存储
void kore_init_cmdline_args(int argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
}

// 获取命令行参数个数
int kore_get_argc(void) {
    return g_argc;
}

// 获取命令行参数数组指针
char **kore_get_argv(void) {
    return g_argv;
}

// 读取文件内容并返回字符串
// 返回 NULL 表示读取失败
char* kore_read_file(const char *path) {
    FILE *file = fopen(path, "r");
    if (!file) {
        return NULL;
    }

    // 获取文件大小
    fseek(file, 0, SEEK_END);
    long size = ftell(file);
    fseek(file, 0, SEEK_SET);

    // 分配内存并读取
    char *content = (char*)malloc(size + 1);
    if (!content) {
        fclose(file);
        return NULL;
    }

    fread(content, 1, size, file);
    content[size] = '\0';
    fclose(file);

    return content;
}

// 将字符串写入文件
// 返回 0 表示成功，-1 表示失败
int kore_write_file(const char *path, const char *content) {
    FILE *file = fopen(path, "w");
    if (!file) {
        return -1;
    }

    size_t len = strlen(content);
    size_t written = fwrite(content, 1, len, file);
    fclose(file);

    return (written == len) ? 0 : -1;
}
