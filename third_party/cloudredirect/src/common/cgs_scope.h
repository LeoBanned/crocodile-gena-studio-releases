// CGS local-only integration. Only a root Lua with a depot manifest reference
// may enter the redirected app set. Depot/DLC IDs without their own Lua fail.
#pragma once
#include <filesystem>
#include <fstream>
#include <string>
#include <cctype>

inline bool CgsManifestGame(const std::string& steam, uint32_t app) {
    if (!app) return false;
    const auto root = std::filesystem::u8path(steam);
    for (const char* dir : {"config/lua", "config/stplug-in"}) {
        const auto lua = root / dir / (std::to_string(app) + ".lua");
        std::error_code ec;
        if (std::filesystem::is_symlink(lua, ec)) continue;
        std::ifstream file(lua);
        if (!file) continue;
        bool self = false, manifest = false;
        std::string line;
        while (std::getline(file, line)) {
            if (line.size() >= 3 && line.substr(0,3) == "\xEF\xBB\xBF") line.erase(0,3);
            std::string compact;
            for (unsigned char c : line) if (!std::isspace(c)) compact += char(std::tolower(c));
            const std::string marker = "addappid(" + std::to_string(app);
            if (compact.starts_with(marker + ")") || compact.starts_with(marker + ",")) self = true;
            // Commented manifest pins also belong to auto-updating games.
            auto pos = compact.find("setmanifestid(");
            if (pos == std::string::npos) continue;
            pos += 14;
            std::string numbers[2];
            for (auto& number : numbers) {
                while (pos < compact.size() && !std::isdigit((unsigned char)compact[pos])) ++pos;
                while (pos < compact.size() && std::isdigit((unsigned char)compact[pos])) number += compact[pos++];
            }
            if (numbers[0].empty() || numbers[1].empty()) continue;
            // Steam may evict its download cache. The Lua retains the manifest
            // association, so a cache cleanup must not switch cloud providers.
            manifest = true;
        }
        if (self && manifest) return true;
    }
    return false;
}
