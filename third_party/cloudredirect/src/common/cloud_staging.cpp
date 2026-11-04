#include "cloud_staging.h"

#include "cloud_metadata_paths.h"

#include <limits>

namespace CloudStorage {
namespace {

bool ParseU32Strict(const std::string& s, uint32_t& out) {
    if (s.empty()) return false;
    for (char c : s) {
        if (c < '0' || c > '9') return false;
    }
    try {
        unsigned long long v = std::stoull(s);
        if (v > (std::numeric_limits<uint32_t>::max)()) return false;
        out = static_cast<uint32_t>(v);
        return true;
    } catch (...) {
        return false;
    }
}

bool IsStagingBlobPath(uint32_t accountId, const std::string& path) {
    size_t p1 = path.find('/');
    if (p1 == std::string::npos || p1 == 0) return false;
    uint32_t parsedAccount = 0;
    if (!ParseU32Strict(path.substr(0, p1), parsedAccount)) return false;
    if (parsedAccount != accountId) return false;

    size_t p2 = path.find('/', p1 + 1);
    if (p2 == std::string::npos || p2 == p1 + 1) return false;
    uint32_t parsedApp = 0;
    if (!ParseU32Strict(path.substr(p1 + 1, p2 - p1 - 1), parsedApp)) return false;

    const std::string staging = "/staging/";
    if (path.compare(p2, staging.size(), staging) != 0) return false;
    size_t batchStart = p2 + staging.size();
    size_t batchEnd = path.find('/', batchStart);
    if (batchEnd == std::string::npos || batchEnd == batchStart) return false;
    uint32_t parsedBatch = 0;
    if (!ParseU32Strict(path.substr(batchStart, batchEnd - batchStart), parsedBatch)) return false;

    const std::string blobs = "/blobs/";
    if (path.compare(batchEnd, blobs.size(), blobs) != 0) return false;
    return batchEnd + blobs.size() < path.size();
}

} // namespace

std::vector<std::string> ClassifyStaleStagingBlobs(
    uint32_t accountId,
    const std::vector<ICloudProvider::FileInfo>& files,
    uint64_t nowUnix,
    uint64_t minAgeSeconds) {
    std::vector<std::string> stale;
    for (const auto& file : files) {
        if (!IsStagingBlobPath(accountId, file.path)) continue;
        if (file.modifiedTime == 0) continue;
        if (file.modifiedTime > nowUnix) continue;
        if (nowUnix - file.modifiedTime < minAgeSeconds) continue;
        stale.push_back(file.path);
    }
    return stale;
}

} // namespace CloudStorage
