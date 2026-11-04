#pragma once
// Cloud-storage paths for CloudRedirect's internal metadata blobs.
// Split out of cloud_intercept.h so cloud_storage and other low-level
// modules can consume just the path constants without pulling in the
// full intercept-layer interface (CNetPacket, hook installers, etc.).

#include <cstdint>
#include <string>
#include <string_view>

namespace CloudIntercept {

// Account-scoped metadata under synthetic appId=0 (never collides with real apps).
inline constexpr uint32_t kAccountScopeAppId = 0;

// Legacy per-app paths (cleanup only; do NOT use for new writes).
inline constexpr const char* kPlaytimeMetadataPath = ".cloudredirect/Playtime.bin";
inline constexpr const char* kStatsMetadataPath    = ".cloudredirect/UserGameStats.bin";

// Even-older paths from the very first builds, before metadata was even
// namespaced under .cloudredirect/. Recognized by the legacy-metadata cleanup
// pass for users who skipped multiple versions.
inline constexpr const char* kLegacyPlaytimeMetadataPath = "Playtime.bin";
inline constexpr const char* kLegacyStatsMetadataPath    = "UserGameStats.bin";

// Per-app metadata filenames (local storage + cloud sync)
// Use .cloudredirect extension to avoid collision with game save filenames.
inline constexpr const char* kManifestFilename     = "manifest.cloudredirect";
inline constexpr const char* kCNFilename           = "cn.cloudredirect";
inline constexpr const char* kFileTokensFilename   = "file_tokens.cloudredirect";
inline constexpr const char* kRootTokenFilename    = "root_token.cloudredirect";
inline constexpr const char* kDeletedFilename      = "deleted.cloudredirect";

// Legacy filenames (for migration - read old, write new)
inline constexpr const char* kLegacyManifestFilename   = "manifest.dat";
inline constexpr const char* kLegacyCNFilename         = "cn.dat";
inline constexpr const char* kLegacyFileTokensFilename = "file_tokens.dat";
inline constexpr const char* kLegacyRootTokenFilename  = "root_token.dat";
inline constexpr const char* kLegacyDeletedFilename    = "deleted.dat";

// Account-scope filename for an app's playtime blob.
inline std::string AccountPlaytimeFilename(uint32_t appId) {
    return "Playtime/" + std::to_string(appId) + ".bin";
}

// Account-scope filename for an app's UserGameStats blob.
inline std::string AccountStatsFilename(uint32_t appId) {
    return "UserGameStats/" + std::to_string(appId) + ".bin";
}

// True if filename is internal metadata.
inline bool IsInternalMetadataFile(std::string_view cleanName) {
    return cleanName == kPlaytimeMetadataPath ||
           cleanName == kStatsMetadataPath ||
           cleanName == kLegacyPlaytimeMetadataPath ||
           cleanName == kLegacyStatsMetadataPath ||
           cleanName == kManifestFilename ||
           cleanName == kLegacyManifestFilename ||
           cleanName == kCNFilename ||
           cleanName == kLegacyCNFilename ||
           cleanName == kFileTokensFilename ||
           cleanName == kLegacyFileTokensFilename ||
           cleanName == kRootTokenFilename ||
           cleanName == kLegacyRootTokenFilename ||
            cleanName == kDeletedFilename ||
            cleanName == kLegacyDeletedFilename;
}

// Reserved names: metadata files, `.cloudredirect` segments/suffixes.
inline bool IsReservedBlobFilename(std::string_view cleanName) {
    if (IsInternalMetadataFile(cleanName)) return true;

    constexpr std::string_view kReservedSegment = ".cloudredirect";
    size_t segmentStart = 0;
    while (segmentStart < cleanName.size()) {
        size_t segmentEnd = cleanName.find_first_of("/\\", segmentStart);
        std::string_view segment = cleanName.substr(
            segmentStart,
            segmentEnd == std::string_view::npos
                ? cleanName.size() - segmentStart
                : segmentEnd - segmentStart);

        if (segment == kReservedSegment) return true;
        if (segmentEnd == std::string_view::npos) {
            return segment.size() > kReservedSegment.size() &&
                   segment.substr(segment.size() - kReservedSegment.size()) == kReservedSegment;
        }
        segmentStart = segmentEnd + 1;
    }

    return false;
}

// Composite key for per-(account, app) maps. High 32 bits = accountId,
// low 32 bits = appId.
inline uint64_t MakeAppAccountKey(uint32_t accountId, uint32_t appId) {
    return (static_cast<uint64_t>(accountId) << 32) | appId;
}

} // namespace CloudIntercept
