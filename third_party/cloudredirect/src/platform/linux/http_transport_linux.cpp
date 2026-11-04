
#include "cloud_provider_base.h"
#include "log.h"

#include <cctype>
#include <cerrno>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <unistd.h>
#include <vector>

#include <curl/curl.h>

static constexpr size_t kMaxResponseSize = 64 * 1024 * 1024;

static bool g_curlInitDone = false;
static std::mutex g_curlInitMutex;

static std::string g_caBundle;
static std::string g_caCertDir;

// curl_easy_init/cleanup can race internal OpenSSL global tables.
static std::mutex g_curlHandleMutex;

// Probe well-known CA bundle locations.
// Environment variables take priority, then standard paths.
static void ProbeSystemCaBundle() {
    for (const char* var : {"SSL_CERT_FILE", "CURL_CA_BUNDLE"}) {
        const char* v = getenv(var);
        if (v && v[0] && access(v, R_OK) == 0) {
            g_caBundle = v;
            LOG("[HTTP] CA bundle from %s: %s", var, v);
            return;
        }
    }

    static const char* const kCaBundlePaths[] = {
        "/etc/ssl/certs/ca-certificates.crt",               // Debian/Ubuntu/Arch/Alpine/Gentoo/Void/NixOS
        "/etc/pki/tls/certs/ca-bundle.crt",                 // Fedora/RHEL/CentOS
        "/etc/ssl/ca-bundle.pem",                           // openSUSE
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",// Fedora alt
        "/etc/ssl/cert.pem",                                // Alpine/Arch/Void symlink
        "/etc/ca-certificates/extracted/tls-ca-bundle.pem", // Arch p11-kit
    };

    for (const char* path : kCaBundlePaths) {
        if (access(path, R_OK) == 0) {
            g_caBundle = path;
            LOG("[HTTP] CA bundle: %s", path);
            break;
        }
    }

    for (const char* var : {"SSL_CERT_DIR"}) {
        const char* v = getenv(var);
        if (v && v[0] && access(v, R_OK) == 0) {
            g_caCertDir = v;
            LOG("[HTTP] CA dir from %s: %s", var, v);
            return;
        }
    }
    static const char* const kCaDirPaths[] = {
        "/etc/ssl/certs",
        "/etc/pki/tls/certs",
    };
    for (const char* path : kCaDirPaths) {
        if (access(path, R_OK) == 0) {
            g_caCertDir = path;
            break;
        }
    }

    if (g_caBundle.empty() && g_caCertDir.empty())
        LOG("[HTTP] WARNING: no system CA bundle found; TLS verification may fail");
}

static bool InitCurl() {
    std::lock_guard<std::mutex> lock(g_curlInitMutex);
    if (g_curlInitDone) return true;

    CURLcode rc = curl_global_init(CURL_GLOBAL_ALL);
    if (rc != CURLE_OK) {
        LOG("[HTTP] curl_global_init failed: %d", (int)rc);
        return false;
    }

    const char* ver = curl_version();
    LOG("[HTTP] Linked libcurl %s (static, OpenSSL)", ver ? ver : "unknown");

    ProbeSystemCaBundle();
    g_curlInitDone = true;
    return true;
}

static size_t WriteCallback(const char* data, size_t size, size_t nmemb, std::string* out) {
    size_t total = size * nmemb;
    if (out->size() + total > kMaxResponseSize) return 0;
    out->append(data, total);
    return total;
}

static size_t HeaderCallback(const char* data, size_t size, size_t nmemb, std::string* out) {
    size_t total = size * nmemb;
    out->append(data, total);
    return total;
}

static std::string ExtractLocation(const std::string& headers) {
    for (const char* key : {"Location: ", "location: "}) {
        size_t pos = headers.find(key);
        if (pos == std::string::npos) continue;
        pos += strlen(key);
        size_t end = headers.find("\r\n", pos);
        if (end == std::string::npos) end = headers.find("\n", pos);
        if (end != std::string::npos) return headers.substr(pos, end - pos);
    }
    return {};
}

static void ParseHeaders(const std::string& raw, std::map<std::string, std::string>& out) {
    size_t pos = 0;
    while (pos < raw.size()) {
        size_t eol = raw.find('\n', pos);
        std::string line = raw.substr(pos, (eol == std::string::npos ? raw.size() : eol) - pos);
        pos = (eol == std::string::npos) ? raw.size() : eol + 1;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        size_t colon = line.find(':');
        if (colon == std::string::npos) continue;
        std::string name = line.substr(0, colon);
        for (char& c : name) c = (char)tolower((unsigned char)c);
        size_t vs = colon + 1;
        while (vs < line.size() && (line[vs] == ' ' || line[vs] == '\t')) vs++;
        out[name] = line.substr(vs);
    }
}

static bool IsCurlTlsFailure(CURLcode res) {
    switch (res) {
        case CURLE_SSL_CONNECT_ERROR:
        case CURLE_SSL_CERTPROBLEM:
        case CURLE_SSL_CIPHER:
        case CURLE_PEER_FAILED_VERIFICATION:
        case CURLE_USE_SSL_FAILED:
        case CURLE_SSL_ENGINE_INITFAILED:
        case CURLE_SSL_CACERT_BADFILE:
        case CURLE_SSL_ISSUER_ERROR:
        case CURLE_SSL_PINNEDPUBKEYNOTMATCH:
        case CURLE_SSL_INVALIDCERTSTATUS:
            return true;
        default:
            return false;
    }
}

static HttpUtil::HttpResp CurlRequest(const char* logTag, const char* method,
                                       const std::string& url, const std::string& body,
                                       const std::vector<std::string>& hdrs,
                                       long timeout, bool captureHeaders,
                                       std::string* outLocation,
                                       bool followRedirects = false,
                                       const TransportOptions* opts = nullptr) {
    HttpUtil::HttpResp resp;

    if (!InitCurl()) {
        LOG("%s libcurl not available", logTag);
        return resp;
    }

    bool allowHttp = opts && opts->allowInsecureHttp;
    if (url.substr(0, 8) != "https://" && !(allowHttp && url.substr(0, 7) == "http://")) {
        LOG("%s BLOCKED non-HTTPS: %s", logTag, url.c_str());
        return resp;
    }

    std::string safeUrl;
    safeUrl.reserve(url.size());
    for (char c : url) {
        if (c == ' ') safeUrl += "%20";
        else safeUrl += c;
    }

    CURL* curl;
    {
        std::lock_guard<std::mutex> lock(g_curlHandleMutex);
        curl = curl_easy_init();
    }
    if (!curl) return resp;

    std::string responseBody;
    std::string responseHeaders;

    curl_easy_setopt(curl, CURLOPT_URL, safeUrl.c_str());
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, (void*)WriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &responseBody);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, timeout);
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 5L);
    curl_easy_setopt(curl, CURLOPT_USERAGENT, "CloudRedirect/1.0");
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, followRedirects ? 1L : 0L);
    if (followRedirects)
        curl_easy_setopt(curl, CURLOPT_MAXREDIRS, 10L);

    if (captureHeaders) {
        curl_easy_setopt(curl, CURLOPT_HEADERFUNCTION, (void*)HeaderCallback);
        curl_easy_setopt(curl, CURLOPT_HEADERDATA, &responseHeaders);
    }

    // TLS: per-provider overrides first, then the probed system bundle.
    bool tlsRelaxed = opts && opts->allowInsecureTls;
    if (tlsRelaxed) {
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 0L);
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYHOST, 0L);
    } else if (opts && !opts->caCertPath.empty()) {
        curl_easy_setopt(curl, CURLOPT_CAINFO, opts->caCertPath.c_str());
    } else {
        if (!g_caBundle.empty())
            curl_easy_setopt(curl, CURLOPT_CAINFO, g_caBundle.c_str());
        if (!g_caCertDir.empty())
            curl_easy_setopt(curl, CURLOPT_CAPATH, g_caCertDir.c_str());
    }

    if (strcmp(method, "GET") != 0)
        curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, method);

    if (strcmp(method, "HEAD") == 0)
        curl_easy_setopt(curl, CURLOPT_NOBODY, 1L);

    if (!body.empty()) {
        curl_easy_setopt(curl, CURLOPT_POSTFIELDS, body.c_str());
        curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE, (long)body.size());
    }

    struct curl_slist* slist = nullptr;
    for (const auto& h : hdrs)
        slist = curl_slist_append(slist, h.c_str());
    if (slist)
        curl_easy_setopt(curl, CURLOPT_HTTPHEADER, slist);

    CURLcode res = curl_easy_perform(curl);

    long httpCode = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &httpCode);

    if (slist) curl_slist_free_all(slist);
    {
        std::lock_guard<std::mutex> lock(g_curlHandleMutex);
        curl_easy_cleanup(curl);
    }

    if (res != CURLE_OK) {
        resp.tlsFailure = IsCurlTlsFailure(res);
        LOG("%s curl failed: %d (%s %s)", logTag, (int)res, method, url.c_str());
        return resp;
    }

    resp.status = (int)httpCode;
    resp.body = std::move(responseBody);

    if (!responseHeaders.empty()) {
        std::string loc = ExtractLocation(responseHeaders);
        if (outLocation) *outLocation = loc;
        resp.location = std::move(loc);
        ParseHeaders(responseHeaders, resp.headers);
    }

    return resp;
}

class StaticCurlTransport : public IHttpTransport {
public:
    explicit StaticCurlTransport(const char* logTag) : m_logTag(logTag) {}

    bool Init() override { return InitCurl(); }
    void Shutdown() override {}
    bool IsReady() const override { return g_curlInitDone; }
    void SetOptions(const TransportOptions& opts) override { m_opts = opts; }

    HttpUtil::HttpResp Request(const char* method, const char* host,
                               const std::string& path, const std::string& body,
                               const std::vector<std::string>& headers) override {
        std::string url = Scheme() + host + path;
        return CurlRequest(m_logTag, method, url, body, headers, 30L, true, nullptr,
                           false, &m_opts);
    }

    HttpUtil::HttpResp RequestUrl(const char* method, const std::string& fullUrl,
                                   const std::string& body,
                                   const std::vector<std::string>& headers) override {
        return CurlRequest(m_logTag, method, fullUrl, body, headers, 60L, true, nullptr,
                           false, &m_opts);
    }

    HttpUtil::HttpResp AuthenticatedGetWithRedirect(const std::string& host,
                                                     const std::string& path,
                                                     const std::string& authHeader) override {
        std::string url = Scheme() + host + path;
        std::vector<std::string> hdrs = {authHeader};
        std::string location;
        auto resp = CurlRequest(m_logTag, "GET", url, {}, hdrs, 30L, true, &location,
                                false, &m_opts);
        if (resp.status >= 300 && resp.status < 400 && !location.empty())
            return CurlRequest(m_logTag, "GET", location, {}, {}, 60L, false, nullptr,
                               /*followRedirects=*/true, &m_opts);
        return resp;
    }

    const char* m_logTag;

private:
    std::string Scheme() const {
        return m_opts.allowInsecureHttp ? "http://" : "https://";
    }
    TransportOptions m_opts;
};

std::unique_ptr<IHttpTransport> CreateHttpTransport(const char* logTag) {
    return std::make_unique<StaticCurlTransport>(logTag);
}
