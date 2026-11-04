#pragma once
// File utilities.

#include "platform.h"
#include "path_util.h"
#include <string>
#include <filesystem>

namespace FileUtil {

// Path encoding
inline std::filesystem::path Utf8ToPath(const std::string& utf8) {
    return Platform().Utf8ToPath(utf8);
}

inline std::string PathToUtf8(const std::filesystem::path& p) {
    return Platform().PathToUtf8(p);
}

inline std::string WideToUtf8(const wchar_t* w) {
    return Platform().WideToUtf8(w);
}

inline std::string WideToUtf8(const wchar_t* w, size_t len) {
    return Platform().WideToUtf8(w, len);
}

// File I/O
inline bool AtomicWriteBinary(const std::string& path, const void* data, size_t len) {
    return Platform().AtomicWriteBinary(path, data, len);
}

inline bool AtomicWriteText(const std::string& path, const std::string& content) {
    return Platform().AtomicWriteText(path, content);
}

// Long path support (\\?\ prefix on Windows, no-op on Linux)
inline std::filesystem::path LongPath(const std::filesystem::path& p) {
#ifdef _WIN32
    const auto& s = p.native();
    if (s.size() >= 3 && s[1] == L':' && (s[2] == L'\\' || s[2] == L'/')) {
        if (s.size() < 4 || s.substr(0, 4) != L"\\\\?\\") {
            std::wstring fixed = s;
            for (auto& c : fixed) if (c == L'/') c = L'\\';
            return std::filesystem::path(L"\\\\?\\" + fixed);
        }
    }
#endif
    return p;
}

// Path validation
inline bool IsPathWithin(const std::string& root, const std::string& fullPath) {
    return Platform().IsPathWithin(root, fullPath);
}

inline bool IsPathRedirectingReparsePoint(const std::string& path) {
    return Platform().IsPathRedirectingReparsePoint(path);
}

// Directory cleanup
inline void CleanupEmptyDirsUpTo(const std::string& startDir, const std::string& stopAt) {
    Platform().CleanupEmptyDirsUpTo(startDir, stopAt);
}

// Cryptographic hashing
inline std::vector<uint8_t> SHA1(const void* data, size_t len) {
    return Platform().SHA1(data, len);
}

} // namespace FileUtil

// Platform constants - available as macros for compatibility
#define kPathSep Platform().PathSeparator()
#define kPathSepStr Platform().PathSeparatorStr()
