#include "r2_provider.h"
#include "log.h"

#include <cctype>

// R2 account IDs are 32 lowercase hex chars; anything else yields a nonexistent
// host that aborts the TLS handshake as an opaque transport error.
static bool IsValidAccountId(const std::string& id) {
    if (id.size() != 32) return false;
    for (char c : id) {
        if (!std::isxdigit(static_cast<unsigned char>(c))) return false;
        if (std::isupper(static_cast<unsigned char>(c))) return false;
    }
    return true;
}

static std::string ExtractJsonString(const std::string& json, const char* key) {
    std::string needle = std::string("\"") + key + "\"";
    size_t k = json.find(needle);
    if (k == std::string::npos) return {};
    size_t colon = json.find(':', k + needle.size());
    if (colon == std::string::npos) return {};
    size_t q1 = json.find('"', colon + 1);
    if (q1 == std::string::npos) return {};
    std::string out;
    for (size_t i = q1 + 1; i < json.size(); ++i) {
        char c = json[i];
        if (c == '\\' && i + 1 < json.size()) { out.push_back(json[++i]); continue; }
        if (c == '"') break;
        out.push_back(c);
    }
    return out;
}

bool R2Provider::ParseExtraCredentials(const std::string& json) {
    m_accountId = ExtractJsonString(json, "account_id");
    return true;
}

std::string R2Provider::DefaultEndpoint() const {
    if (m_accountId.empty()) {
        LOG("[R2] account_id missing from credentials; set it to your 32-character "
            "Cloudflare account ID, or set \"endpoint\" for a custom host");
        return {};
    }
    if (!IsValidAccountId(m_accountId)) {
        LOG("[R2] account_id is not a 32-character hex Cloudflare account ID "
            "(got %zu chars); check you did not paste an API token or access key",
            m_accountId.size());
        return {};
    }
    return m_accountId + ".r2.cloudflarestorage.com";
}
