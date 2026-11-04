#pragma once
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <string_view>
#include <vector>
#include <mutex>
#include <filesystem>
#include <fstream>

#define WIN32_LEAN_AND_MEAN
// Keep the min/max macros out of the way of std::min/std::max.
#define NOMINMAX
#include <Windows.h>

// kPathSep and kPathSepStr are provided via Platform() in file_util.h
