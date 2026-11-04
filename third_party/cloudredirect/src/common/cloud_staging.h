#pragma once

#include "cloud_storage.h"
#include "cloud_provider.h"

#include <cstdint>
#include <string>
#include <vector>

namespace CloudStorage {

std::vector<std::string> ClassifyStaleStagingBlobs(
    uint32_t accountId,
    const std::vector<ICloudProvider::FileInfo>& files,
    uint64_t nowUnix,
    uint64_t minAgeSeconds);

} // namespace CloudStorage
