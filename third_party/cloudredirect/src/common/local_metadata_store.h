#pragma once
#include <cstdint>
#include <string>
#include <unordered_map>
#include <unordered_set>

namespace LocalMetadataStore {

void Init(const std::string& baseRoot);
void InitApp(uint32_t accountId, uint32_t appId);

// Root tokens
bool SaveRootTokens(uint32_t accountId, uint32_t appId,
    const std::unordered_set<std::string>& tokens);
std::unordered_set<std::string> LoadRootTokens(uint32_t accountId, uint32_t appId);

// File tokens
bool SaveFileTokens(uint32_t accountId, uint32_t appId,
    const std::unordered_map<std::string, std::string>& fileTokens,
    const std::unordered_set<std::string>& validRootTokens = {});
std::unordered_map<std::string, std::string> LoadFileTokens(uint32_t accountId, uint32_t appId,
    const std::unordered_set<std::string>& validRootTokens = {});

} // namespace LocalMetadataStore
