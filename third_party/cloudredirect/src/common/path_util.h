#pragma once
// UTF-8 path-string helpers (no platform/filesystem deps).

#include <string>
#include <cstring>

namespace FileUtil {

inline void NormalizeSlashesInPlace(std::string& s) {
    for (auto& c : s) { if (c == '\\') c = '/'; }
}

// Forward-slash normalized prefix with trailing '/'.
inline std::string MakePathPrefix(std::string dir) {
    NormalizeSlashesInPlace(dir);
    if (!dir.empty() && dir.back() != '/') dir += '/';
    return dir;
}

// Case-insensitive (ASCII) prefix strip. Replaces std::filesystem::relative
// which throws on non-ASCII paths on MSVC. Returns false on mismatch.
inline bool RelativeUtf8Path(const std::string& fullUtf8,
                             const std::string& rootPrefix,
                             std::string* out) {
    if (fullUtf8.size() <= rootPrefix.size()) return false;
#ifdef _WIN32
    if (_strnicmp(fullUtf8.c_str(), rootPrefix.c_str(), rootPrefix.size()) != 0)
        return false;
#else
    if (strncasecmp(fullUtf8.c_str(), rootPrefix.c_str(), rootPrefix.size()) != 0)
        return false;
#endif
    *out = fullUtf8.substr(rootPrefix.size());
    return true;
}

} // namespace FileUtil
