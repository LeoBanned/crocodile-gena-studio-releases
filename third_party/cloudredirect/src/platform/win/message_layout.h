#pragma once
#include <cstddef>
#include <cstdint>

// Derive the wrapper layout from Steam's own slot-5 forwarding code, rather
// than treating a Steam build number as an ABI guarantee. Unknown layouts
// must not install hooks which could parse a header as a save request.
namespace CgsMessageLayout {
struct Layout { uint32_t header = 0, body = 0; };
inline bool Resolve(const uint8_t* code, size_t size, Layout& result) {
    if (!code || size < 15) return false;
    Layout found{};
    for (size_t i = 0; i + 11 <= size; ++i) {
        // mov r9,[r9+body]; mov r8,[r8+body]; call [rax+20h]
        if (code[i] != 0x4d || code[i+1] != 0x8b || code[i+2] != 0x49 ||
            code[i+4] != 0x4d || code[i+5] != 0x8b || code[i+6] != 0x40 ||
            code[i+3] != code[i+7] || code[i+8] != 0xff ||
            code[i+9] != 0x50 || code[i+10] != 0x20) continue;
        const uint32_t body = code[i+3];
        if (body != 0x30 && body != 0x38) return false;
        bool headerFound = false;
        const size_t start = i > 48 ? i - 48 : 0;
        for (size_t j = start; j + 4 <= i; ++j) {
            // mov rax,[r8+header], used for header.routing_appid.
            if (code[j] == 0x49 && code[j+1] == 0x8b && code[j+2] == 0x40 &&
                code[j+3] == body - 8) headerFound = true;
        }
        if (!headerFound || (found.body && found.body != body)) return false;
        found = {body - 8, body};
    }
    if (!found.body) return false;
    result = found;
    return true;
}
}
