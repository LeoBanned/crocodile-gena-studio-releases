#include "../third_party/cloudredirect/src/common/cgs_scope.h"
#include <cassert>
#include <chrono>
#include <cstdint>
#include <iostream>
int main() {
    auto root = std::filesystem::temp_directory_path() / ("cgs-scope-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    std::filesystem::create_directories(root / "config/lua");
    std::filesystem::create_directories(root / "depotcache");
    auto write = [&](const char* file, const char* bytes) { std::ofstream(root / file) << bytes; };
    write("config/lua/10.lua", "addappid(10)\n--setManifestid(11,\"123\")\naddappid(11,0,\"key\")");
    write("config/lua/20.lua", "addappid(21)\nsetManifestid(11,\"123\")");
    write("config/lua/30.lua.disabled", "addappid(30)\nsetManifestid(11,\"123\")");
    auto utf8 = root.u8string(); std::string steam(utf8.begin(),utf8.end());
    assert(CgsManifestGame(steam,10)); // manifest reference survives cache eviction
    write("depotcache/11_123.manifest", "manifest");
    assert(CgsManifestGame(steam,10));
    assert(!CgsManifestGame(steam,11)); // depot/DLC ID
    assert(!CgsManifestGame(steam,20)); // Lua only unlocks a different app
    assert(!CgsManifestGame(steam,30)); // disabled
    assert(!CgsManifestGame(steam,40)); // ordinary Steam game
    write("config/lua/40.lua", "addappid(40)");
    assert(!CgsManifestGame(steam,40)); // Lua without a manifest reference
    std::filesystem::remove(root / "config/lua/10.lua");
    assert(!CgsManifestGame(steam,10)); // removed while Steam remains open
    std::filesystem::remove_all(root);
    std::cout << "Manifest-only native scope passed\n";
}
