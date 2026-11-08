#include "../third_party/cloudredirect/src/platform/win/message_layout.h"
#include <array>
#include <cassert>
#include <cstdio>
#include <fstream>
#include <vector>
int main(int argc, char** argv) {
    using namespace CgsMessageLayout;
    // Forwarding instructions from the installed October Steam build. The old
    // fixed body=0x30 reads its header and serializes an empty cloud request.
    std::array<uint8_t, 27> modern = {
        0x49,0x8b,0x40,0x30, 0x48,0x89,0x7c,0x24,0x20,
        0x8b,0x48,0x74, 0x89,0x0f, 0x49,0x8b,0xca,
        0x4d,0x8b,0x49,0x38, 0x4d,0x8b,0x40,0x38, 0xff,0x50
    };
    std::vector<uint8_t> code(modern.begin(), modern.end()); code.push_back(0x20);
    Layout layout;
    assert(Resolve(code.data(), code.size(), layout));
    assert(layout.header == 0x30 && layout.body == 0x38);
    // Legacy ABI still resolves; no guessed offsets for an unknown build.
    code[3] = 0x28; code[20] = code[24] = 0x30;
    assert(Resolve(code.data(), code.size(), layout));
    assert(layout.header == 0x28 && layout.body == 0x30);
    code[24] = 0x38; assert(!Resolve(code.data(), code.size(), layout));
    code[24] = 0x30; code[3] = 0x20;
    assert(!Resolve(code.data(), code.size(), layout));
    assert(!Resolve(nullptr, 256, layout));
    assert(!Resolve(code.data(), 12, layout));
    // Optional unmodified 256-byte slot-5 sample extracted from Steam PE.
    if (argc == 2) {
        std::ifstream file(argv[1], std::ios::binary);
        std::vector<uint8_t> fixture((std::istreambuf_iterator<char>(file)), {});
        assert(!fixture.empty() && Resolve(fixture.data(), fixture.size(), layout));
        assert(layout.header == 0x30 && layout.body == 0x38);
    }
    puts("PASS: current/legacy wrappers, truncated/unknown/mismatched layouts, installed Steam sample");
}
