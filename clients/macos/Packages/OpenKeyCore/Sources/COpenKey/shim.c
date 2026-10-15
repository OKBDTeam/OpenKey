/*
 * COpenKey 目标的占位编译单元。
 *
 * 本 C 目标本身只提供头文件（openkey.h，cbindgen 产物）与 modulemap；
 * 保留一个 .c 文件以确保目标有编译单元，同时顺带在编译期校验头文件自洽性。
 */
#include "openkey.h"
