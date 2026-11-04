#include "local_metadata_store.h"
#include "cloud_metadata_paths.h"
#include "file_util.h"
#include "log.h"

#include <ctime>
#include <filesystem>
#include <fstream>
#include <shared_mutex>
#include <string>
#include <system_error>
#include <unordered_map>
#include <unordered_set>

using CloudIntercept::kRootTokenFilename;
using CloudIntercept::kLegacyRootTokenFilename;
using CloudIntercept::kFileTokensFilename;
using CloudIntercept::kLegacyFileTokensFilename;

namespace LocalMetadataStore {

static std::string g_baseRoot;
static std::shared_mutex g_metadataMutex;

static void RemoveLegacyMetadataIfPresent(const std::string& legacyPath,
                                           const char* legacyName,
                                           uint32_t appId) {
    std::error_code ec;
    bool removed = std::filesystem::remove(FileUtil::Utf8ToPath(legacyPath), ec);
    if (ec && ec != std::errc::no_such_file_or_directory) {
        LOG("RemoveLegacyMetadataIfPresent: failed to remove %s for app %u: %s",
            legacyName, appId, ec.message().c_str());
    } else if (removed) {
        LOG("RemoveLegacyMetadataIfPresent: removed %s for app %u",
            legacyName, appId);
    }
}

static std::string GetAppPathInternal(uint32_t accountId, uint32_t appId) {
#ifdef _WIN32
    return g_baseRoot + std::to_string(accountId) + "\\" + std::to_string(appId) + "\\";
#else
    return g_baseRoot + std::to_string(accountId) + "/" + std::to_string(appId) + "/";
#endif
}

void Init(const std::string& baseRoot) {
    g_baseRoot = baseRoot;
#ifdef _WIN32
    if (!g_baseRoot.empty() && g_baseRoot.back() != '\\')
        g_baseRoot += '\\';
#else
    if (!g_baseRoot.empty() && g_baseRoot.back() != '/')
        g_baseRoot += '/';
#endif
    std::error_code ec;
    std::filesystem::create_directories(FileUtil::Utf8ToPath(g_baseRoot), ec);
    if (ec) {
        LOG("LocalMetadataStore Init: create_directories failed for '%s': %s",
            g_baseRoot.c_str(), ec.message().c_str());
    }
    LOG("LocalMetadataStore initialized at: %s", g_baseRoot.c_str());
}

void InitApp(uint32_t accountId, uint32_t appId) {
    auto appPath = GetAppPathInternal(accountId, appId);
    std::error_code ec;
    std::filesystem::create_directories(FileUtil::Utf8ToPath(appPath), ec);
    if (ec) {
        LOG("LocalMetadataStore InitApp: create_directories failed for '%s': %s",
            appPath.c_str(), ec.message().c_str());
    }
    LOG("LocalMetadataStore: account %u app %u path: %s", accountId, appId, appPath.c_str());
}

bool SaveRootTokens(uint32_t accountId, uint32_t appId, const std::unordered_set<std::string>& tokens) {
    std::lock_guard<std::shared_mutex> lock(g_metadataMutex);
    std::string appDir = GetAppPathInternal(accountId, appId);
    std::error_code dirEc;
    std::filesystem::create_directories(FileUtil::Utf8ToPath(appDir), dirEc);
    if (dirEc) {
        LOG("SaveRootTokens: create_directories failed for '%s': %s",
            appDir.c_str(), dirEc.message().c_str());
        return false;
    }
    std::string path = appDir + kRootTokenFilename;
    std::string content;
    for (auto& t : tokens) {
        content += t + "\n";
    }
    if (FileUtil::AtomicWriteText(path, content)) {
        LOG("SaveRootTokens: persisted %zu tokens for app %u", tokens.size(), appId);
        RemoveLegacyMetadataIfPresent(appDir + kLegacyRootTokenFilename,
                                      kLegacyRootTokenFilename, appId);
        return true;
    } else {
        LOG("SaveRootTokens: failed for app %u", appId);
        return false;
    }
}

std::unordered_set<std::string> LoadRootTokens(uint32_t accountId, uint32_t appId) {
    std::unordered_set<std::string> tokens;
    bool needsRewrite = false;
    bool fromLegacy = false;

    // Read phase: shared lock allows concurrent readers
    {
        std::shared_lock<std::shared_mutex> lock(g_metadataMutex);
        std::string appPath = GetAppPathInternal(accountId, appId);
        std::string path = appPath + kRootTokenFilename;
        std::string legacyPath = appPath + kLegacyRootTokenFilename;

        std::ifstream f(FileUtil::Utf8ToPath(path));
        if (!f) {
            f.open(FileUtil::Utf8ToPath(legacyPath));
            if (f) fromLegacy = true;
        }
        if (f) {
            std::string line;
            while (std::getline(f, line)) {
                std::string original = line;
                while (!line.empty() && (line.back() == '\r' || line.back() == '\n'))
                    line.pop_back();
                if (line != original)
                    needsRewrite = true;
                if (!line.empty()) {
                    tokens.insert(line);
                }
            }
            f.close();
            if (!tokens.empty()) {
                LOG("LoadRootTokens: loaded %zu tokens from %s for app %u",
                    tokens.size(), fromLegacy ? "legacy path" : "disk", appId);
            }
        }
    }

    // Migrate from legacy or rewrite if corrupted.
        // Skip if canonical file already exists.
    if ((fromLegacy || needsRewrite) && !tokens.empty()) {
        if (fromLegacy) {
            LOG("LoadRootTokens: migrating tokens for app %u", appId);
        } else {
            LOG("LoadRootTokens: cleaning corrupted tokens for app %u", appId);
        }
        (void)SaveRootTokens(accountId, appId, tokens);
    }

    return tokens;
}

bool SaveFileTokens(uint32_t accountId, uint32_t appId,
                    const std::unordered_map<std::string, std::string>& fileTokens,
                    const std::unordered_set<std::string>& validRootTokens) {
    std::lock_guard<std::shared_mutex> lock(g_metadataMutex);
    std::string appDir = GetAppPathInternal(accountId, appId);
    std::error_code dirEc;
    std::filesystem::create_directories(FileUtil::Utf8ToPath(appDir), dirEc);
    if (dirEc) {
        LOG("SaveFileTokens: create_directories failed for '%s': %s",
            appDir.c_str(), dirEc.message().c_str());
        return false;
    }

    // Always save to disk; load-time filtering handles correctness.
    std::unordered_map<std::string, std::string> toSave;
    toSave.reserve(fileTokens.size());
    size_t rejectedCount = 0;
    for (const auto& [cleanName, token] : fileTokens) {
        if (validRootTokens.empty() || token.empty() || validRootTokens.count(token) ||
            CloudIntercept::IsInternalMetadataFile(cleanName)) {
            toSave[cleanName] = token;
        } else {
            LOG("SaveFileTokens: app %u WARNING '%s' -> '%s' (root token not in valid set -- saving anyway)",
                appId, cleanName.c_str(), token.c_str());
            toSave[cleanName] = token;
            ++rejectedCount;
        }
    }
    if (rejectedCount > 0)
        LOG("SaveFileTokens: app %u warning: %zu entries with unverified root tokens saved", appId, rejectedCount);

    std::string path = appDir + kFileTokensFilename;
    std::string content;
    for (auto& [cleanName, token] : toSave) {
        content += cleanName + "\t" + token + "\n";
    }
    if (FileUtil::AtomicWriteText(path, content)) {
        LOG("SaveFileTokens: persisted %zu entries for app %u", toSave.size(), appId);
        RemoveLegacyMetadataIfPresent(appDir + kLegacyFileTokensFilename,
                                      kLegacyFileTokensFilename, appId);
        return true;
    } else {
        LOG("SaveFileTokens: failed for app %u", appId);
        return false;
    }
}

std::unordered_map<std::string, std::string> LoadFileTokens(uint32_t accountId, uint32_t appId,
                                                            const std::unordered_set<std::string>& validRootTokens) {
    std::shared_lock<std::shared_mutex> lock(g_metadataMutex);
    std::string appPath = GetAppPathInternal(accountId, appId);
    std::string path = appPath + kFileTokensFilename;
    std::string legacyPath = appPath + kLegacyFileTokensFilename;
    std::unordered_map<std::string, std::string> result;
    bool fromLegacy = false;

    std::ifstream f(FileUtil::Utf8ToPath(path));
    if (!f) {
        f.open(FileUtil::Utf8ToPath(legacyPath));
        if (f) fromLegacy = true;
    }
    if (f) {
        std::string line;
        while (std::getline(f, line)) {
            while (!line.empty() && (line.back() == '\r' || line.back() == '\n'))
                line.pop_back();
            if (line.empty()) continue;
            auto tab = line.find('\t');
            if (tab == std::string::npos) continue;
            std::string cleanName = line.substr(0, tab);
            std::string token = line.substr(tab + 1);
            if (!cleanName.empty()) {
                result[cleanName] = token;
            }
        }
        f.close();
        if (!result.empty()) {
            LOG("LoadFileTokens: loaded %zu entries from %s for app %u",
                result.size(), fromLegacy ? "legacy path" : "disk", appId);
        }
    }

    // Migrate from legacy if needed. Skip if canonical file already
    // exists (prior session or concurrent thread may have migrated).
    if (fromLegacy && !result.empty()) {
        std::string canonicalPath = appPath + kFileTokensFilename;
        if (!std::filesystem::exists(FileUtil::Utf8ToPath(canonicalPath))) {
            // Re-check under exclusive lock.
            lock.unlock();
            // Re-check existence to mitigate TOCTOU with concurrent migration.
            if (!std::filesystem::exists(FileUtil::Utf8ToPath(canonicalPath))) {
                LOG("LoadFileTokens: migrating file tokens for app %u", appId);
                SaveFileTokens(accountId, appId, result, validRootTokens);
            }
        }
    }

    // Filter against valid root tokens and metadata files.
    if (validRootTokens.empty()) return result;
    std::unordered_map<std::string, std::string> filtered;
    filtered.reserve(result.size());
    for (auto& [cleanName, token] : result) {
        if (token.empty() || validRootTokens.count(token) || CloudIntercept::IsInternalMetadataFile(cleanName)) {
            filtered[cleanName] = token;
        }
    }
    return filtered;
}

} // namespace LocalMetadataStore
