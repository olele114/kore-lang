/**
 * 命令行参数运行时支持
 *
 * 提供访问 main(argc, argv) 参数的函数。
 */

#include <stddef.h>

// 全局变量存储命令行参数
static int g_argc = 0;
static char **g_argv = NULL;

/**
 * 初始化命令行参数存储
 *
 * 由编译器生成的 main 函数入口调用
 */
void kore_init_cmdline_args(int argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
}

/**
 * 获取命令行参数个数
 *
 * @return 参数个数（包括程序名）
 */
int kore_get_argc(void) {
    return g_argc;
}

/**
 * 获取命令行参数数组指针
 *
 * @return char** 指向参数字符串数组的指针
 */
char **kore_get_argv(void) {
    return g_argv;
}
