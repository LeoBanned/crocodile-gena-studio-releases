#include "legacy_metadata_cleanup.h"
#include "cloud_metadata_paths.h"
#include "file_util.h"
#include "log.h"

#include <array>
#include <exception>
#include <filesystem>
#include <string_view>
#include <system_error>

namespace LegacyMetadataCleanup {

namespace fs = std::filesystem;

namespace {

// Kept in sync with cloud_intercept.h's kLegacy*MetadataPath via a unit test.
constexpr std::array<std::string_view, 2> kLegacyNames = {
    "Playtime.bin",
    "UserGameStats.bin",
};

// Canonical subdir under the private blob cache.
constexpr std::string_view kCanonicalDir = ".cloudredirect";

struct MetadataPair {
    std::string_view canonical;
    std::string_view legacy;
};

constexpr std::array<MetadataPair, 5> kLegacyMetadataPairs = {{
    {CloudIntercept::kCNFilename, CloudIntercept::kLegacyCNFilename},
    {CloudIntercept::kManifestFilename, CloudIntercept::kLegacyManifestFilename},
    {CloudIntercept::kFileTokensFilename, CloudIntercept::kLegacyFileTokensFilename},
    {CloudIntercept::kRootTokenFilename, CloudIntercept::kLegacyRootTokenFilename},
    {CloudIntercept::kDeletedFilename, CloudIntercept::kLegacyDeletedFilename},
}};

bool FileExistsNoSymlink(const fs::path& p) {
    std::error_code ec;
    auto st = fs::symlink_status(p, ec);
    return !ec && !fs::is_symlink(st) && fs::is_regular_file(st);
}

bool TombstoneTargetsOnlyObsoleteInternalMetadata(const fs::path& p) {
    std::ifstream f(p, std::ios::binary);
    if (!f) return false;

    std::string line;
    bool sawEntry = false;
    while (std::getline(f, line)) {
        while (!line.empty() && (line.back() == '\r' || line.back() == '\n')) {
            line.pop_back();
        }
        if (line.empty()) continue;
        auto tab = line.find('\t');
        std::string filename = (tab == std::string::npos) ? line : line.substr(0, tab);
        if (filename.empty()) continue;
        sawEntry = true;
        if (filename != CloudIntercept::kPlaytimeMetadataPath &&
            filename != CloudIntercept::kStatsMetadataPath &&
            filename != CloudIntercept::kLegacyPlaytimeMetadataPath &&
            filename != CloudIntercept::kLegacyStatsMetadataPath) {
            return false;
        }
    }
    return sawEntry;
}

// remove() that swallows ENOENT but reports real failures via stats.errors.
bool RemoveFileIfPresent(const fs::path& p, SweepStats& stats) {
    std::error_code ec;
    if (!fs::exists(p, ec)) return false;
    if (fs::remove(p, ec)) {
        stats.filesRemoved++;
        LOG("[LegacyCleanup] removed %s", FileUtil::PathToUtf8(p).c_str());
        return true;
    }
    if (ec) {
        stats.errors++;
        LOG("[LegacyCleanup] failed to remove %s: %s",
            FileUtil::PathToUtf8(p).c_str(), ec.message().c_str());
    }
    return false;
}

// Remove dir + contents. Reparse points are unlinked, never descended:
// a junction at .cloudredirect\ could redirect deletion to arbitrary trees.
bool RemoveDirIfPresent(const fs::path& dir, SweepStats& stats) {
    std::error_code ec;
    auto symStat = fs::symlink_status(dir, ec);
    if (ec || !fs::exists(symStat)) return false;
    if (fs::is_symlink(symStat)) {
        if (fs::remove(dir, ec)) {
            stats.dirsRemoved++;
            LOG("[LegacyCleanup] unlinked reparse point %s (did not follow)",
                FileUtil::PathToUtf8(dir).c_str());
            return true;
        }
        if (ec) {
            stats.errors++;
            LOG("[LegacyCleanup] failed to unlink reparse point %s: %s",
                FileUtil::PathToUtf8(dir).c_str(), ec.message().c_str());
        }
        return false;
    }
    if (!fs::is_directory(symStat)) {
        // Name belongs to something else; leave it alone.
        return false;
    }
    // remove_all is atomic vs. concurrent Steam writes during login.
    std::uintmax_t removed = fs::remove_all(dir, ec);
    if (ec) {
        stats.errors++;
        LOG("[LegacyCleanup] failed to remove dir %s: %s",
            FileUtil::PathToUtf8(dir).c_str(), ec.message().c_str());
        return false;
    }
    if (removed > 0) {
        // Subtract the dir itself from the file count.
        stats.filesRemoved += static_cast<int>(removed - 1);
        stats.dirsRemoved++;
        LOG("[LegacyCleanup] removed dir %s (%llu file(s)/subdir(s) inside)",
            FileUtil::PathToUtf8(dir).c_str(),
            static_cast<unsigned long long>(removed - 1));
        return true;
    }
    return false;
}

// Enumerate numeric-name subdirs (account/app IDs); ignore everything else.
std::vector<fs::path> ListNumericSubdirs(const fs::path& parent) {
    std::vector<fs::path> out;
    std::error_code ec;
    if (!fs::is_directory(parent, ec)) return out;
    fs::directory_iterator it(parent, fs::directory_options::skip_permission_denied, ec);
    fs::directory_iterator end;
    if (ec) return out;
    while (it != end) {
        std::error_code entryEc;
        // symlink_status: skip symlinks at account/app level.
        auto st = it->symlink_status(entryEc);
        if (!entryEc && !fs::is_symlink(st) && fs::is_directory(st)) {
            const std::string name = FileUtil::PathToUtf8(it->path().filename());
            bool allDigits = !name.empty();
            for (char c : name) {
                if (c < '0' || c > '9') { allDigits = false; break; }
            }
            if (allDigits) out.push_back(it->path());
        }
        std::error_code stepEc;
        it.increment(stepEc);
        if (stepEc) break;
    }
    return out;
}

} // namespace

// Scrub *.cloudredirect and legacy *.dat metadata from userdata/remote/ (contamination from older builds or SteamTools sync)
static bool IsScrubbableMetadataInRemoteDir(const fs::path& filePath) {
    const std::string filename = FileUtil::PathToUtf8(filePath.filename());

    constexpr std::string_view kExt = ".cloudredirect";
    if (filename.size() > kExt.size() &&
        filename.compare(filename.size() - kExt.size(), kExt.size(), kExt) == 0) {
        return true;
    }

    if (filename == CloudIntercept::kLegacyCNFilename ||
        filename == CloudIntercept::kLegacyManifestFilename ||
        filename == CloudIntercept::kLegacyFileTokensFilename ||
        filename == CloudIntercept::kLegacyRootTokenFilename ||
        filename == CloudIntercept::kLegacyDeletedFilename) {
        return true;
    }

    return false;
}

SweepStats PruneSteamUserdata(const std::string& steamPath) {
    SweepStats stats;
    if (steamPath.empty()) return stats;

    try {
        // Utf8ToPath: avoid ACP narrowing on non-ASCII profiles.
        fs::path userdata = FileUtil::Utf8ToPath(steamPath) / "userdata";
        std::error_code ec;
        if (!fs::is_directory(userdata, ec)) return stats;

        for (const auto& acctDir : ListNumericSubdirs(userdata)) {
            for (const auto& appDir : ListNumericSubdirs(acctDir)) {
                fs::path remoteDir = appDir / "remote";
                if (!fs::is_directory(remoteDir, ec)) continue;

                // Top-level legacy files directly inside remote\.
                for (auto name : kLegacyNames) {
                    RemoveFileIfPresent(remoteDir / std::string(name), stats);
                }

                // .cloudredirect\ here is a leftover from a mid-evolution DLL.
                RemoveDirIfPresent(remoteDir / std::string(kCanonicalDir), stats);

                fs::directory_iterator dirIt(remoteDir,
                    fs::directory_options::skip_permission_denied, ec);
                fs::directory_iterator dirEnd;
                if (ec) continue;
                while (dirIt != dirEnd) {
                    std::error_code entryEc;
                    auto st = dirIt->symlink_status(entryEc);
                    if (entryEc) {
                        std::error_code stepEc;
                        dirIt.increment(stepEc);
                        if (stepEc) break;
                        continue;
                    }
                    // Don't follow symlinks
                    if (fs::is_regular_file(st) || fs::is_symlink(st)) {
                        const fs::path& p = dirIt->path();
                        if (IsScrubbableMetadataInRemoteDir(p)) {
                            RemoveFileIfPresent(p, stats);
                        }
                    }
                    std::error_code stepEc;
                    dirIt.increment(stepEc);
                    if (stepEc) break;
                }
            }
        }
    } catch (const std::exception& ex) {
        stats.errors++;
        LOG("[LegacyCleanup] userdata sweep aborted on exception: %s", ex.what());
    } catch (...) {
        stats.errors++;
        LOG("[LegacyCleanup] userdata sweep aborted on unknown exception");
    }

    if (stats.filesRemoved > 0 || stats.dirsRemoved > 0 || stats.errors > 0) {
        LOG("[LegacyCleanup] userdata sweep: %d file(s) removed, %d dir(s) removed, %d error(s)",
            stats.filesRemoved, stats.dirsRemoved, stats.errors);
    }
    return stats;
}

SweepStats PruneLocalBlobCache(const std::string& localRoot) {
    SweepStats stats;
    if (localRoot.empty()) return stats;

    try {
        fs::path storage = FileUtil::Utf8ToPath(localRoot) / "storage";
        std::error_code ec;
        if (!fs::is_directory(storage, ec)) return stats;

        for (const auto& acctDir : ListNumericSubdirs(storage)) {
            for (const auto& appDir : ListNumericSubdirs(acctDir)) {
                fs::path canonDir = appDir / std::string(kCanonicalDir);
                for (auto name : kLegacyNames) {
                    fs::path legacy = appDir / std::string(name);
                    fs::path canon = canonDir / std::string(name);

                    // Require canonical sibling; symlink_status to refuse reparse points.
                    std::error_code legEc, canEc;
                    auto legSt = fs::symlink_status(legacy, legEc);
                    auto canSt = fs::symlink_status(canon, canEc);
                    if (legEc || fs::is_symlink(legSt) || !fs::is_regular_file(legSt)) continue;
                    if (canEc || fs::is_symlink(canSt) || !fs::is_regular_file(canSt)) continue;

                    RemoveFileIfPresent(legacy, stats);
                }
            }
        }
    } catch (const std::exception& ex) {
        stats.errors++;
        LOG("[LegacyCleanup] local cache sweep aborted on exception: %s", ex.what());
    } catch (...) {
        stats.errors++;
        LOG("[LegacyCleanup] local cache sweep aborted on unknown exception");
    }

    if (stats.filesRemoved > 0 || stats.errors > 0) {
        LOG("[LegacyCleanup] local cache sweep: %d file(s) removed, %d error(s)",
            stats.filesRemoved, stats.errors);
    }
    return stats;
}

SweepStats PruneLocalLegacyAppMetadata(const std::string& localRoot) {
    SweepStats stats;
    if (localRoot.empty()) return stats;

    try {
        fs::path storage = FileUtil::Utf8ToPath(localRoot) / "storage";
        std::error_code ec;
        if (!fs::is_directory(storage, ec)) return stats;

        for (const auto& acctDir : ListNumericSubdirs(storage)) {
            for (const auto& appDir : ListNumericSubdirs(acctDir)) {
                for (const auto& pair : kLegacyMetadataPairs) {
                    if (pair.canonical == CloudIntercept::kDeletedFilename) {
                        continue;
                    }
                    fs::path canonical = appDir / std::string(pair.canonical);
                    fs::path legacy = appDir / std::string(pair.legacy);
                    if (FileExistsNoSymlink(canonical) && FileExistsNoSymlink(legacy)) {
                        RemoveFileIfPresent(legacy, stats);
                    }
                }

                fs::path canonicalDeleted = appDir / std::string(CloudIntercept::kDeletedFilename);
                fs::path legacyDeleted = appDir / std::string(CloudIntercept::kLegacyDeletedFilename);
                if (FileExistsNoSymlink(canonicalDeleted) &&
                    TombstoneTargetsOnlyObsoleteInternalMetadata(canonicalDeleted)) {
                    RemoveFileIfPresent(legacyDeleted, stats);
                    RemoveFileIfPresent(canonicalDeleted, stats);
                }
            }
        }
    } catch (const std::exception& ex) {
        stats.errors++;
        LOG("[LegacyCleanup] local metadata sweep aborted on exception: %s", ex.what());
    } catch (...) {
        stats.errors++;
        LOG("[LegacyCleanup] local metadata sweep aborted on unknown exception");
    }

    if (stats.filesRemoved > 0 || stats.errors > 0) {
        LOG("[LegacyCleanup] local metadata sweep: %d file(s) removed, %d error(s)",
            stats.filesRemoved, stats.errors);
    }
    return stats;
}

SweepStats PruneAutoCloudPollutionRoots(const std::vector<std::string>& roots) {
    SweepStats stats;
    if (roots.empty()) return stats;

    try {
        for (const auto& rawRoot : roots) {
            if (rawRoot.empty()) continue;

            fs::path rootPath = FileUtil::Utf8ToPath(rawRoot);
            std::error_code ec;
            if (!fs::is_directory(rootPath, ec)) continue;

            fs::path canonDir = rootPath / std::string(kCanonicalDir);

            // symlink_status + junction check; junctions are the unprivileged primitive.
            std::error_code symEc;
            auto symSt = fs::symlink_status(canonDir, symEc);
            if (symEc || !fs::exists(symSt)) continue;

            const bool isReparsePoint =
#ifdef _WIN32
                fs::is_symlink(symSt) || symSt.type() == fs::file_type::junction;
#else
                fs::is_symlink(symSt);
#endif
            if (isReparsePoint) {
                std::error_code rmEc;
                if (fs::remove(canonDir, rmEc)) {
                    stats.dirsRemoved++;
                    LOG("[LegacyCleanup] unlinked reparse point %s (did not follow)",
                        FileUtil::PathToUtf8(canonDir).c_str());
                } else if (rmEc) {
                    stats.errors++;
                    LOG("[LegacyCleanup] failed to unlink reparse point %s: %s",
                        FileUtil::PathToUtf8(canonDir).c_str(), rmEc.message().c_str());
                }
                continue;
            }

            if (!fs::is_directory(symSt)) continue;

            // Only the two named files; parent root is user-shared.
            for (auto name : kLegacyNames) {
                fs::path victim = canonDir / std::string(name);
                std::error_code vEc;
                auto vSt = fs::symlink_status(victim, vEc);
                if (vEc || !fs::exists(vSt)) continue;
#ifdef _WIN32
                if (fs::is_symlink(vSt) || vSt.type() == fs::file_type::junction) continue;
#else
                if (fs::is_symlink(vSt)) continue;
#endif
                if (!fs::is_regular_file(vSt)) continue;
                RemoveFileIfPresent(victim, stats);
            }

            // rmdir only if empty.
            std::error_code emptyEc;
            if (fs::is_empty(canonDir, emptyEc) && !emptyEc) {
                std::error_code rmEc;
                if (fs::remove(canonDir, rmEc)) {
                    stats.dirsRemoved++;
                    LOG("[LegacyCleanup] removed empty dir %s", FileUtil::PathToUtf8(canonDir).c_str());
                } else if (rmEc) {
                    stats.errors++;
                    LOG("[LegacyCleanup] failed to remove empty dir %s: %s",
                        FileUtil::PathToUtf8(canonDir).c_str(), rmEc.message().c_str());
                }
            }
        }
    } catch (const std::exception& ex) {
        stats.errors++;
        LOG("[LegacyCleanup] autocloud pollution sweep aborted on exception: %s", ex.what());
    } catch (...) {
        stats.errors++;
        LOG("[LegacyCleanup] autocloud pollution sweep aborted on unknown exception");
    }

    if (stats.filesRemoved > 0 || stats.dirsRemoved > 0 || stats.errors > 0) {
        LOG("[LegacyCleanup] autocloud pollution sweep: %d file(s) removed, %d dir(s) removed, %d error(s)",
            stats.filesRemoved, stats.dirsRemoved, stats.errors);
    }
    return stats;
}

std::vector<std::string> ClassifyLegacyCloudBlobsToDelete(
    const std::vector<std::string>& cloudBlobRawPaths) {
    std::vector<std::string> toDelete;
    if (cloudBlobRawPaths.empty()) return toDelete;

    for (const auto& raw : cloudBlobRawPaths) {
        static const std::string kBlobs = "/blobs/";
        auto pos = raw.find(kBlobs);
        if (pos == std::string::npos) continue;
        std::string filename = raw.substr(pos + kBlobs.size());
        if (filename.empty()) continue;

        for (auto leg : kLegacyNames) {
            if (filename == leg || filename == std::string(kCanonicalDir) + "/" + std::string(leg)) {
                toDelete.push_back(raw);
                break;
            }
        }
    }

    return toDelete;
}

} // namespace LegacyMetadataCleanup
