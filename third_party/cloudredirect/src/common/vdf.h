#pragma once
#include <string>
#include <string_view>
#include <functional>

namespace VdfUtil {

struct FieldInfo {
    std::string_view key;
    std::string_view value;
    size_t valStart;    // byte offset of value text (between quotes)
    size_t valEnd;      // byte offset of closing quote
};

// Callback receives each key-value field found inside the target section.
// Return false from callback to stop iteration early.
using FieldCallback = std::function<bool(const FieldInfo&)>;

// Navigate a text VDF to a nested section path (e.g. {"Software","Valve","Steam","Apps","12345"})
// and invoke the callback for each key-value pair in that section.
// Returns true if the section was found, false otherwise.
bool ForEachFieldInSection(const std::string& vdfContent,
                           const char* const* sectionPath, int pathLen,
                           FieldCallback cb);

// Locate nested VDF section body. Returns [sectionStart, sectionEnd) or false.
bool FindVdfSectionRange(const std::string& vdfContent,
                         const char* const* sectionPath, size_t pathLen,
                         size_t& sectionStart, size_t& sectionEnd);

// Iterate direct children (scalars and sub-sections). Return false to stop.
using ChildCallback = std::function<bool(std::string_view name)>;
bool ForEachChildInSection(const std::string& vdfContent,
                           const char* const* sectionPath, size_t pathLen,
                           ChildCallback cb);

} // namespace VdfUtil
