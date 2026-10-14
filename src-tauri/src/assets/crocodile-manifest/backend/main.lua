local fs = require("fs")
local http = require("http")
local json = require("json")
local logger = require("logger")
local millennium = require("millennium")
local utils = require("utils")

local RYUU_DOWNLOAD_BASE = "https://generator.ryuu.lol/api/download/"
local HUBCAP_BASE = "https://hubcapmanifest.com/api/v1"
local USER_AGENT =
"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"
local HISTORY_LIMIT = 50
local REMOTE_VERSION_TTL = 15
local HUBCAP_USAGE_TTL = 15
local version_cache = {}
local hubcap_usage_cache = nil

local function reply(success, message, extra)
    local payload = extra or {}
    payload.success = success
    payload.message = message
    return json.encode(payload)
end

local function is_windows()
    return jit.os == "Windows"
end

local function powershell_quote(value)
    return "'" .. tostring(value):gsub("'", "''") .. "'"
end

local function read_binary(path)
    return utils.read_file(path)
end

local function write_binary(path, data)
    return utils.write_file(path, data)
end

local function manifest_cache_directory(appid)
    local local_app_data = utils.getenv("LOCALAPPDATA")
    if not local_app_data or local_app_data == "" then
        return nil
    end
    return fs.join(local_app_data, "CrocodileManifest", "manifest-cache", tostring(appid))
end

local function safe_manifest_name(value)
    local name = tostring(value or "")
    if name == "" or fs.filename(name) ~= name or not name:lower():match("%.manifest$") then
        return nil
    end
    return name
end

local function state_file()
    local local_app_data = utils.getenv("LOCALAPPDATA")
    if not local_app_data or local_app_data == "" then
        return nil
    end
    return fs.join(local_app_data, "CrocodileManifest", "state.json")
end

local function default_state()
    return {
        cardMode = "full",
        service = "ryuu",
        ryuuKey = "",
        hubcapKey = "",
        history = {},
        installedGames = {},
        installedVersions = {}
    }
end

local function load_state()
    local path = state_file()
    if not path or not fs.is_file(path) then
        return default_state()
    end
    local content = read_binary(path)
    if not content or content == "" then
        return default_state()
    end
    local ok, decoded = pcall(json.decode, content)
    if not ok or type(decoded) ~= "table" then
        logger:error("Crocodile Manifest state file is invalid")
        return default_state()
    end
    if type(decoded.history) ~= "table" then
        decoded.history = {}
    end
    if type(decoded.installedVersions) ~= "table" then
        decoded.installedVersions = {}
    end
    if type(decoded.installedGames) ~= "table" then
        decoded.installedGames = {}
    end
    if decoded.cardMode ~= "full" and decoded.cardMode ~= "button" and decoded.cardMode ~= "crocodile" then
        decoded.cardMode = decoded.compactMode == true and "button" or "full"
    end
    if decoded.service ~= "ryuu" and decoded.service ~= "hubcap" then
        decoded.service = "ryuu"
    end
    if type(decoded.ryuuKey) ~= "string" then
        decoded.ryuuKey = ""
    end
    if type(decoded.hubcapKey) ~= "string" then
        decoded.hubcapKey = ""
    end
    decoded.compactMode = nil
    return decoded
end

local function save_state(state)
    local path = state_file()
    if not path then
        return false, "LOCALAPPDATA недоступна"
    end
    local directory = fs.join(utils.getenv("LOCALAPPDATA"), "CrocodileManifest")
    local created, create_err = fs.create_directories(directory)
    if created == nil then
        return false, tostring(create_err)
    end
    return write_binary(path, json.encode(state))
end

local function add_history(appid, name, status, message, extra)
    local state = load_state()
    local entry = {
        appid = appid,
        name = name ~= "" and name or ("AppID " .. appid),
        status = status,
        message = message,
        timestamp = os.time(),
        pendingRestart = status == "success" and (not extra or extra.pendingRestart ~= false),
        luaCount = extra and tonumber(extra.luaCount) or 0,
        manifestCount = extra and tonumber(extra.manifestCount) or 0,
        manifestFiles = extra and extra.manifestFiles or {},
        packageVersion = extra and tostring(extra.packageVersion or "") or "",
        service = extra and tostring(extra.service or "") or ""
    }
    if status == "success" and entry.packageVersion ~= "" then
        state.installedVersions[appid] = entry.packageVersion
    end
    if status == "success" then
        state.installedGames[appid] = {
            appid = appid,
            name = entry.name,
            installedAt = entry.timestamp,
            luaCount = entry.luaCount,
            manifestCount = entry.manifestCount,
            manifestFiles = entry.manifestFiles,
            packageVersion = entry.packageVersion,
            service = entry.service
        }
    end
    table.insert(state.history, 1, entry)
    while #state.history > HISTORY_LIMIT do
        table.remove(state.history)
    end
    local ok, err = save_state(state)
    if not ok then
        logger:error("Could not save Crocodile Manifest history: " .. tostring(err))
    end
end

local function latest_history(state, appid)
    for _, entry in ipairs(state.history) do
        if tostring(entry.appid) == appid then
            return entry
        end
    end
    return nil
end

local function installed_lua_path(steam_path, appid)
    local directory = fs.join(steam_path, "config", "lua")
    local enabled = fs.join(directory, appid .. ".lua")
    if fs.is_file(enabled) then
        return enabled
    end
    local disabled = fs.join(directory, appid .. ".lua.disabled")
    if fs.is_file(disabled) then
        return disabled
    end
    return nil
end

local function recorded_manifest_files(steam_path, appid, installed_game, latest)
    local result = {}
    local seen = {}

    local function add(value)
        local name = safe_manifest_name(value)
        local key = name and name:lower() or nil
        if name and not seen[key] then
            seen[key] = true
            table.insert(result, name)
        end
    end

    -- Older plugin versions did not always persist manifestFiles. Merge both
    -- state sources instead of letting an empty installedGames entry hide history.
    local function merge(source)
        if type(source) == "table" then
            for _, value in pairs(source) do
                add(value)
            end
        end
    end
    merge(type(installed_game) == "table" and installed_game.manifestFiles or nil)
    merge(type(latest) == "table" and latest.manifestFiles or nil)

    -- The installed Lua remains the source of truth even when state.json is old.
    local lua_path = installed_lua_path(steam_path, appid)
    if lua_path then
        local content = read_binary(lua_path)
        if content then
            for depot, manifest in content:gmatch("[sS][eE][tT][mM][aA][nN][iI][fF][eE][sS][tT][iI][dD][^%d]*(%d+)[^%d]+(%d+)") do
                add(depot .. "_" .. manifest .. ".manifest")
            end
        end
    end

    -- Finally discover backups themselves. This repairs installs created by
    -- versions which cached files successfully but failed to record their list.
    local cache_dir = manifest_cache_directory(appid)
    if cache_dir and fs.is_directory(cache_dir) then
        local entries = fs.list_recursive(cache_dir)
        if entries then
            for _, entry in ipairs(entries) do
                if entry.is_file then
                    add(fs.filename(entry.path))
                end
            end
        end
    end

    table.sort(result)
    return result
end

-- Steam can remove depot manifests when a download is cancelled. Keep a copy
-- outside depotcache and put it back before reporting the game as installed.
local function sync_manifest_cache(steam_path, appid, manifest_files)
    local result = {
        expectedCount = #manifest_files,
        missingBefore = 0,
        restoredCount = 0,
        missingCount = 0,
        missingFiles = {}
    }
    if #manifest_files == 0 then
        return result
    end

    local cache_dir = manifest_cache_directory(appid)
    if not cache_dir then
        result.missingCount = #manifest_files
        result.missingFiles = manifest_files
        return result
    end
    local created, create_err = fs.create_directories(cache_dir)
    if created == nil then
        logger:error("Could not prepare manifest cache: " .. tostring(create_err))
    end

    local depotcache = fs.join(steam_path, "depotcache")
    fs.create_directories(depotcache)
    for _, name in ipairs(manifest_files) do
        local target = fs.join(depotcache, name)
        local backup = fs.join(cache_dir, name)
        if fs.is_file(target) then
            if not fs.is_file(backup) then
                local content, read_err = read_binary(target)
                if content then
                    local ok, write_err = write_binary(backup, content)
                    if not ok then
                        logger:error("Could not cache " .. name .. ": " .. tostring(write_err))
                    end
                else
                    logger:error("Could not read " .. name .. " for cache: " .. tostring(read_err))
                end
            end
        else
            result.missingBefore = result.missingBefore + 1
            if fs.is_file(backup) then
                local content, read_err = read_binary(backup)
                if content then
                    local ok, write_err = write_binary(target, content)
                    if ok then
                        result.restoredCount = result.restoredCount + 1
                        logger:info("Restored Steam manifest " .. name .. " for AppID " .. tostring(appid))
                    else
                        logger:error("Could not restore " .. name .. ": " .. tostring(write_err))
                    end
                else
                    logger:error("Could not read cached " .. name .. ": " .. tostring(read_err))
                end
            end
            if not fs.is_file(target) then
                result.missingCount = result.missingCount + 1
                table.insert(result.missingFiles, name)
            end
        end
    end
    return result
end

local function service_key(state, service)
    if service == "hubcap" then
        return utils.trim(tostring(state.hubcapKey or ""))
    end
    return utils.trim(tostring(state.ryuuKey or ""))
end

local function request_headers(service, key)
    if service == "hubcap" then
        return {
            ["Authorization"] = "Bearer " .. key,
            ["Accept"] = "application/zip, application/octet-stream, */*",
            ["Accept-Encoding"] = "identity"
        }
    end
    return {
        ["X-Auth-Key"] = key,
        ["Accept"] = "application/zip, application/octet-stream, */*",
        ["Accept-Encoding"] = "identity",
        ["Cache-Control"] = "no-cache",
        ["Pragma"] = "no-cache",
        ["Referer"] = "https://generator.ryuu.lol/"
    }
end

local function header_value(headers, wanted)
    if type(headers) ~= "table" then
        return nil
    end
    local lower_wanted = wanted:lower()
    for key, value in pairs(headers) do
        if tostring(key):lower() == lower_wanted then
            local text = utils.trim(tostring(value or ""))
            if text ~= "" then
                return text
            end
        end
    end
    return nil
end

local function map_service_status(status, service)
    local name = service == "hubcap" and "HubCap" or "Ryuu"
    if status == 401 or status == 403 then
        return "Неверный или просроченный API-ключ " .. name
    elseif status == 404 then
        return "Для этой игры пока нет манифестов в " .. name
    elseif status == 429 then
        return service == "hubcap"
            and "Достигнут дневной лимит HubCap"
            or "Слишком много запросов к Ryuu. Попробуйте позже"
    end
    return name .. " вернул ошибку " .. tostring(status)
end

local function fetch_hubcap_usage(key, force)
    if key == "" then
        return false, "Укажите API-ключ HubCap в настройках плагина"
    end
    if not force and hubcap_usage_cache and hubcap_usage_cache.key == key
        and os.time() - hubcap_usage_cache.checkedAt < HUBCAP_USAGE_TTL then
        return true, hubcap_usage_cache.usage
    end

    local response, request_err = http.request(HUBCAP_BASE .. "/user/stats", {
        method = "GET",
        headers = {
            ["Authorization"] = "Bearer " .. key,
            ["Accept"] = "application/json"
        },
        timeout = 15,
        follow_redirects = true,
        verify_ssl = true,
        user_agent = USER_AGENT
    })
    if not response then
        return false, "HubCap недоступен: " .. tostring(request_err)
    end
    local status = tonumber(response.status) or 0
    if status < 200 or status >= 300 then
        return false, map_service_status(status, "hubcap")
    end
    local ok, decoded = pcall(json.decode, tostring(response.body or ""))
    if not ok or type(decoded) ~= "table" then
        return false, "HubCap вернул некорректные данные лимита"
    end
    local used = tonumber(decoded.daily_usage or decoded.dailyUsage or 0) or 0
    local limit = tonumber(decoded.daily_limit or decoded.dailyLimit or 25) or 25
    local remaining = tonumber(decoded.remaining)
    if remaining == nil then
        remaining = math.max(0, limit - used)
    end
    local usage = {
        used = used,
        limit = limit,
        remaining = math.max(0, remaining),
        checkedAt = os.time()
    }
    hubcap_usage_cache = {
        key = key,
        checkedAt = os.time(),
        usage = usage
    }
    return true, usage
end

local function fetch_remote_version(appid, force, service, key)
    if service ~= "ryuu" or key == "" then
        return nil
    end
    local cache_key = service .. ":" .. appid
    local cached = version_cache[cache_key]
    if not force and cached and os.time() - cached.checkedAt < REMOTE_VERSION_TTL then
        return cached.token
    end

    local response = http.request(RYUU_DOWNLOAD_BASE .. appid, {
        method = "HEAD",
        headers = request_headers(service, key),
        timeout = 8,
        follow_redirects = true,
        verify_ssl = true,
        user_agent = USER_AGENT
    })
    local token = nil
    if response and tonumber(response.status) and tonumber(response.status) >= 200 and tonumber(response.status) < 300 then
        local etag = header_value(response.headers, "etag")
        local modified = header_value(response.headers, "last-modified")
        local length = header_value(response.headers, "content-length")
        if etag or modified then
            token = table.concat({ etag or "", modified or "", length or "" }, "|")
        end
    end
    version_cache[cache_key] = {
        token = token,
        checkedAt = os.time()
    }
    return token
end

local function run_powershell(script_path, success_marker)
    local command = 'powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' ..
        script_path:gsub('"', '""') .. '" 2>&1'
    local output, status = utils.exec(command)
    if fs.is_file(success_marker) then
        return true
    end
    local detail = output and utils.trim(output) or ""
    if detail == "" then
        detail = "PowerShell завершился с кодом " .. tostring(status or "неизвестно")
    end
    return false, detail
end

local function patch_lua(content)
    content = content:gsub("\r\n", "\n"):gsub("\r", "\n")
    local output = {}
    for line in (content .. "\n"):gmatch("(.-)\n") do
        if line:match("^%s*setManifestid") then
            line = "--" .. line
        end
        table.insert(output, line)
    end
    return table.concat(output, "\n")
end

local function legacy_manifests_match(appid, service, key, installed_game)
    if service ~= "ryuu" or key == "" then
        return nil
    end

    local steam_path = millennium.steam_path()
    local temp_root = utils.getenv("TEMP") or utils.getenv("TMP")
    if not steam_path or steam_path == "" or not temp_root or temp_root == "" then
        return nil
    end

    local token = tostring(os.time()) .. "-" .. utils.uuid()
    local work_dir = fs.join(temp_root, "ryuu-version-check-" .. appid .. "-" .. token)
    local extract_dir = fs.join(work_dir, "package")
    local zip_path = fs.join(work_dir, appid .. "-manifests.zip")
    local script_path = fs.join(work_dir, "extract.ps1")
    local success_marker = fs.join(work_dir, "extracted.ok")
    local created = fs.create_directories(extract_dir)
    if created == nil then
        return nil
    end

    local download = http.download(RYUU_DOWNLOAD_BASE .. appid .. "?file_type=manifest", zip_path, {
        headers = request_headers(service, key),
        timeout = 45,
        follow_redirects = true,
        verify_ssl = true,
        user_agent = USER_AGENT
    })
    local status = download and (tonumber(download.status) or 0) or 0
    if not download or not download.success or status < 200
        or status >= 300 or (tonumber(download.bytes_written) or 0) < 4 then
        fs.remove_all(work_dir)
        return nil
    end

    local script = "$ErrorActionPreference = 'Stop'\n" ..
        "Add-Type -AssemblyName System.IO.Compression.FileSystem\n" ..
        "[System.IO.Compression.ZipFile]::ExtractToDirectory(" ..
        powershell_quote(zip_path) .. ", " .. powershell_quote(extract_dir) .. ")\n" ..
        "Set-Content -LiteralPath " .. powershell_quote(success_marker) .. " -Value 'ok' -NoNewline\n"
    local script_ok = write_binary(script_path, script)
    local expanded = false
    if script_ok then
        expanded = run_powershell(script_path, success_marker)
    end
    if not script_ok or not expanded then
        fs.remove_all(work_dir)
        return nil
    end

    local entries = fs.list_recursive(extract_dir)
    if not entries then
        fs.remove_all(work_dir)
        return nil
    end

    local depotcache = fs.join(steam_path, "depotcache")
    local remote_files = {}
    local remote_count = 0
    local matches = true
    for _, entry in ipairs(entries) do
        if entry.is_file then
            local name = fs.filename(entry.path)
            if name:lower():match("%.manifest$") then
                remote_files[name:lower()] = true
                remote_count = remote_count + 1
                local remote_content = read_binary(entry.path)
                local local_content = read_binary(fs.join(depotcache, name))
                if not remote_content or not local_content or remote_content ~= local_content then
                    matches = false
                    break
                end
            end
        end
    end

    if matches and type(installed_game) == "table" and type(installed_game.manifestFiles) == "table" then
        for _, name in pairs(installed_game.manifestFiles) do
            local clean_name = tostring(name or "")
            if clean_name:lower():match("%.manifest$") and not remote_files[clean_name:lower()] then
                matches = false
                break
            end
        end
    end

    fs.remove_all(work_dir)
    if remote_count == 0 then
        return nil
    end
    return matches
end

local function install_package(appid, service, key)
    local steam_path = millennium.steam_path()
    if not steam_path or steam_path == "" then
        return false, "Millennium не смог определить папку Steam"
    end

    local temp_root = utils.getenv("TEMP") or utils.getenv("TMP")
    if not temp_root or temp_root == "" then
        return false, "Не удалось определить временную папку Windows"
    end

    local token = tostring(os.time()) .. "-" .. utils.uuid()
    local service_name = service == "hubcap" and "HubCap" or "Ryuu"
    local download_url = service == "hubcap"
        and (HUBCAP_BASE .. "/manifest/" .. appid)
        or (RYUU_DOWNLOAD_BASE .. appid)
    local work_dir = fs.join(temp_root, service .. "-manifest-" .. appid .. "-" .. token)
    local extract_dir = fs.join(work_dir, "package")
    local zip_path = fs.join(work_dir, appid .. ".zip")
    local script_path = fs.join(work_dir, "extract.ps1")
    local success_marker = fs.join(work_dir, "extracted.ok")

    local created, create_err = fs.create_directories(extract_dir)
    if created == nil then
        return false, "Не удалось подготовить временную папку: " .. tostring(create_err)
    end

    -- Ryuu does not always expose response headers through http.download.
    -- Capture the server version before downloading so the installed package
    -- is never silently associated with a newer version on the first status check.
    local package_version = ""
    if service == "ryuu" then
        package_version = tostring(fetch_remote_version(appid, true, service, key) or "")
    end

    local download, download_err = http.download(download_url, zip_path, {
        headers = request_headers(service, key),
        timeout = 90,
        follow_redirects = true,
        verify_ssl = true,
        user_agent = USER_AGENT
    })
    if service == "hubcap" then
        hubcap_usage_cache = nil
    end
    if not download then
        fs.remove_all(work_dir)
        return false, service_name .. " недоступен: " .. tostring(download_err)
    end
    local status = tonumber(download.status) or 0
    if status < 200 or status >= 300 then
        fs.remove_all(work_dir)
        return false, map_service_status(status, service)
    end
    if not download.success or (tonumber(download.bytes_written) or 0) < 4 then
        fs.remove_all(work_dir)
        return false, service_name .. " не смог сохранить архив с манифестом"
    end

    if service == "ryuu" then
        local etag = header_value(download.headers, "etag")
        local modified = header_value(download.headers, "last-modified")
        local length = header_value(download.headers, "content-length")
        if etag or modified then
            package_version = table.concat({ etag or "", modified or "", length or "" }, "|")
        end
    end

    local script = "$ErrorActionPreference = 'Stop'\n" ..
        "Add-Type -AssemblyName System.IO.Compression.FileSystem\n" ..
        "[System.IO.Compression.ZipFile]::ExtractToDirectory(" ..
        powershell_quote(zip_path) .. ", " .. powershell_quote(extract_dir) .. ")\n" ..
        "Set-Content -LiteralPath " .. powershell_quote(success_marker) .. " -Value 'ok' -NoNewline\n"
    local script_ok, script_err = write_binary(script_path, script)
    local expanded, powershell_err = false, nil
    if script_ok then
        expanded, powershell_err = run_powershell(script_path, success_marker)
    end
    if not script_ok or not expanded then
        fs.remove_all(work_dir)
        return false,
            "Не удалось распаковать пакет " .. service_name .. ": " ..
            tostring(script_err or powershell_err or "ошибка PowerShell")
    end

    -- Ship the same loader as CrocLua; never report success if DLL/config update failed.
    local backend = fs.join(steam_path, "millennium", "plugins", "crocodile-manifest", "backend")
    local loader_marker = fs.join(work_dir, "loader.ok")
    local loader_script = fs.join(work_dir, "loader.ps1")
    local loader_command = "$ErrorActionPreference = 'Stop'\n& " ..
        powershell_quote(fs.join(backend, "install-loader.ps1")) ..
        " -SteamPath " .. powershell_quote(steam_path) ..
        " -ResourcePath " .. powershell_quote(fs.join(backend, "loader")) .. "\n" ..
        "Set-Content -LiteralPath " .. powershell_quote(loader_marker) .. " -Value 'ok' -NoNewline\n"
    local loader_written, loader_write_error = write_binary(loader_script, loader_command)
    local loader_ready, loader_error = false, nil
    if loader_written then
        loader_ready, loader_error = run_powershell(loader_script, loader_marker)
    end
    if not loader_ready then
        fs.remove_all(work_dir)
        return false, "Не удалось обновить загрузчик. Закройте Steam и обновите плагин через CrocLua: " ..
            tostring(loader_write_error or loader_error)
    end

    local stplugin = fs.join(steam_path, "config", "lua")
    local depotcache = fs.join(steam_path, "depotcache")
    local manifest_cache = manifest_cache_directory(appid)
    local plugin_ok, plugin_err = fs.create_directories(stplugin)
    local depot_ok, depot_err = fs.create_directories(depotcache)
    if plugin_ok == nil or depot_ok == nil then
        fs.remove_all(work_dir)
        return false,
            "Не удалось подготовить папки Steam: " .. tostring(plugin_err or depot_err)
    end

    local entries, list_err = fs.list_recursive(extract_dir)
    if not entries then
        fs.remove_all(work_dir)
        return false, "Не удалось прочитать пакет: " .. tostring(list_err)
    end

    local lua_count = 0
    local manifest_count = 0
    local manifest_files = {}
    local failures = {}
    for _, entry in ipairs(entries) do
        if entry.is_file then
            local name = fs.filename(entry.path)
            local lower = name:lower()
            if lower:match("^%d+%.lua$") then
                local content, read_err = read_binary(entry.path)
                if content then
                    local target = fs.join(stplugin, name)
                    local ok, err = write_binary(target, patch_lua(content))
                    if ok then
                        lua_count = lua_count + 1
                    else
                        table.insert(failures, name .. ": " .. tostring(err))
                    end
                else
                    table.insert(failures, name .. ": " .. tostring(read_err))
                end
            elseif lower:match("%.manifest$") then
                local content, read_err = read_binary(entry.path)
                if content then
                    local target = fs.join(depotcache, name)
                    local ok, err = write_binary(target, content)
                    if ok then
                        manifest_count = manifest_count + 1
                        table.insert(manifest_files, name)
                        if manifest_cache then
                            local cache_ok, cache_err = fs.create_directories(manifest_cache)
                            if cache_ok ~= nil then
                                local saved, save_err = write_binary(fs.join(manifest_cache, name), content)
                                if not saved then
                                    logger:error("Could not cache " .. name .. ": " .. tostring(save_err))
                                end
                            else
                                logger:error("Could not prepare manifest cache: " .. tostring(cache_err))
                            end
                        end
                    else
                        table.insert(failures, name .. ": " .. tostring(err))
                    end
                else
                    table.insert(failures, name .. ": " .. tostring(read_err))
                end
            end
        end
    end

    fs.remove_all(work_dir)

    if lua_count == 0 then
        return false, "В пакете " .. service_name .. " не найден Lua-файл"
    end
    if #failures > 0 then
        return false, "Часть файлов не установлена: " .. table.concat(failures, "; ")
    end
    return true, "Файлы автоматически размещены в папках Steam", {
        luaCount = lua_count,
        manifestCount = manifest_count,
        manifestFiles = manifest_files,
        service = service,
        packageVersion = package_version
    }
end

function install_manifest(appid, name)
    local clean = tostring(appid or ""):match("^(%d+)$")
    if not clean then
        return reply(false, "Некорректный AppID")
    end
    if not is_windows() then
        return reply(false, "Первая версия плагина поддерживает только Windows")
    end

    local game_name = utils.trim(tostring(name or ""))
    local state = load_state()
    local steam_path = millennium.steam_path()
    local latest = latest_history(state, clean)
    local installed_game = state.installedGames[clean]
    local installed_path = steam_path and installed_lua_path(steam_path, clean) or nil
    local manifest_files = recorded_manifest_files(steam_path, clean, installed_game, latest)
    local health = steam_path and sync_manifest_cache(steam_path, clean, manifest_files)
        or { missingBefore = 0, missingCount = 0 }
    local repairing = installed_path ~= nil and health.missingBefore > 0
    local service = state.service == "hubcap" and "hubcap" or "ryuu"
    local key = service_key(state, service)
    if key == "" then
        local service_name = service == "hubcap" and "HubCap" or "Ryuu"
        return reply(false, "Укажите личный API-ключ " .. service_name .. " в настройках плагина", {
            service = service,
            configured = false
        })
    end

    logger:info("Crocodile Manifest is downloading data for AppID " .. clean .. " via " .. service)
    local ok, message, extra = install_package(clean, service, key)
    if not ok then
        logger:error(message)
        add_history(clean, game_name, "error", message)
        return reply(false, message, {
            service = service,
            configured = true
        })
    end
    extra.repaired = repairing
    extra.pendingRestart = not repairing
    add_history(clean, game_name, "success", message, extra)
    logger:info("Crocodile Manifest installed data for AppID " .. clean)
    extra.configured = true
    return reply(true, message, extra)
end

function get_game_status(appid)
    local clean = tostring(appid or ""):match("^(%d+)$")
    if not clean then
        return reply(false, "Некорректный AppID")
    end
    local steam_path = millennium.steam_path()
    if not steam_path or steam_path == "" then
        return reply(false, "Millennium не смог определить папку Steam")
    end

    local state = load_state()
    local service = state.service == "hubcap" and "hubcap" or "ryuu"
    local key = service_key(state, service)
    local configured = key ~= ""
    local latest = latest_history(state, clean)
    local installed_path = installed_lua_path(steam_path, clean)
    local installed_game = state.installedGames[clean]
    local manifest_files = recorded_manifest_files(steam_path, clean, installed_game, latest)
    local manifest_health = sync_manifest_cache(steam_path, clean, manifest_files)
    local game_state = configured and "available" or "configuration_required"
    local update_available = false
    if installed_path then
        if manifest_health.missingCount > 0 then
            game_state = configured and "repair_required" or "configuration_required"
        elseif latest and latest.status == "success" and latest.pendingRestart == true then
            game_state = "pending_restart"
        else
            -- A store page is opened explicitly by the user, so always make a
            -- fresh lightweight HEAD request instead of trusting the old cache.
            local remote_version = fetch_remote_version(clean, true, service, key)
            local installed_version = tostring(state.installedVersions[clean] or "")
            local trusted_version = type(installed_game) == "table"
                and tostring(installed_game.packageVersion or "") or ""
            if remote_version and trusted_version == "" then
                local matches = legacy_manifests_match(clean, service, key, installed_game)
                if matches == false then
                    game_state = "update_available"
                    update_available = true
                elseif matches == true then
                    state.installedVersions[clean] = remote_version
                    installed_game.packageVersion = remote_version
                    save_state(state)
                    game_state = "installed"
                else
                    game_state = "installed"
                end
            elseif remote_version and installed_version ~= remote_version then
                game_state = "update_available"
                update_available = true
            else
                game_state = "installed"
            end
        end
    elseif configured and latest and latest.status == "error" then
        game_state = "error"
    end

    local response = {
        state = game_state,
        cardMode = state.cardMode,
        service = service,
        configured = configured,
        installed = installed_path ~= nil,
        updateAvailable = update_available,
        lastMessage = latest and tostring(latest.message or "") or "",
        lastTimestamp = latest and tonumber(latest.timestamp) or 0,
        luaCount = latest and tonumber(latest.luaCount) or 0,
        manifestCount = latest and tonumber(latest.manifestCount) or 0,
        missingManifestCount = manifest_health.missingCount,
        restoredManifestCount = manifest_health.restoredCount
    }
    if service == "hubcap" and configured then
        local usage_ok, usage_or_error = fetch_hubcap_usage(key, false)
        if usage_ok then
            response.hubcapUsage = usage_or_error
        else
            response.hubcapUsageError = tostring(usage_or_error)
        end
    end
    return reply(true, "Статус определён", response)
end

function restore_manifest_cache()
    local steam_path = millennium.steam_path()
    if not steam_path or steam_path == "" then
        return reply(false, "Millennium не смог определить папку Steam")
    end

    local state = load_state()
    local restored_count = 0
    local missing_count = 0
    for appid, installed_game in pairs(state.installedGames) do
        local latest = latest_history(state, tostring(appid))
        local manifest_files = recorded_manifest_files(steam_path, tostring(appid), installed_game, latest)
        local health = sync_manifest_cache(steam_path, tostring(appid), manifest_files)
        restored_count = restored_count + health.restoredCount
        missing_count = missing_count + health.missingCount
    end
    return reply(true, "Резервные копии проверены", {
        restoredCount = restored_count,
        missingCount = missing_count
    })
end

function get_plugin_state()
    local state = load_state()
    local steam_path = millennium.steam_path()
    local changed = false
    if steam_path and steam_path ~= "" then
        for _, entry in ipairs(state.history) do
            local appid = tostring(entry.appid or "")
            if appid:match("^%d+$") and entry.status == "success" and not state.installedGames[appid]
                and installed_lua_path(steam_path, appid) then
                state.installedGames[appid] = {
                    appid = appid,
                    name = tostring(entry.name or ("AppID " .. appid)),
                    installedAt = tonumber(entry.timestamp) or 0,
                    luaCount = tonumber(entry.luaCount) or 0,
                    manifestCount = tonumber(entry.manifestCount) or 0,
                    manifestFiles = type(entry.manifestFiles) == "table" and entry.manifestFiles or {},
                    packageVersion = tostring(entry.packageVersion or ""),
                    service = tostring(entry.service or "")
                }
                changed = true
            end
        end
    end

    local installed_games = {}
    for appid, game in pairs(state.installedGames) do
        if steam_path and installed_lua_path(steam_path, tostring(appid)) then
            table.insert(installed_games, game)
        else
            state.installedGames[appid] = nil
            state.installedVersions[appid] = nil
            changed = true
        end
    end
    table.sort(installed_games, function(a, b)
        return tostring(a.name or a.appid):lower() < tostring(b.name or b.appid):lower()
    end)
    if changed then
        save_state(state)
    end

    return reply(true, "Состояние загружено", {
        cardMode = state.cardMode,
        service = state.service,
        hasRyuuKey = utils.trim(tostring(state.ryuuKey or "")) ~= "",
        hasHubcapKey = utils.trim(tostring(state.hubcapKey or "")) ~= "",
        history = state.history,
        installedGames = installed_games
    })
end

function set_service_settings(payload)
    local decoded_ok, settings = pcall(json.decode, tostring(payload or ""))
    if not decoded_ok or type(settings) ~= "table" then
        return reply(false, "Не удалось прочитать настройки сервиса")
    end
    local clean_service = tostring(settings.service or ""):lower()
    if clean_service ~= "ryuu" and clean_service ~= "hubcap" then
        return reply(false, "Неизвестный сервис манифестов")
    end

    local state = load_state()
    state.service = clean_service
    local clean_ryuu = utils.trim(tostring(settings.ryuuKey or ""))
    local clean_hubcap = utils.trim(tostring(settings.hubcapKey or ""))
    if settings.clearRyuu == true then
        state.ryuuKey = ""
    elseif clean_ryuu ~= "" then
        state.ryuuKey = clean_ryuu
    end
    if settings.clearHubcap == true then
        state.hubcapKey = ""
    elseif clean_hubcap ~= "" then
        state.hubcapKey = clean_hubcap
    end

    local ok, err = save_state(state)
    if not ok then
        return reply(false, "Не удалось сохранить настройки сервиса: " .. tostring(err))
    end
    for cache_key in pairs(version_cache) do
        version_cache[cache_key] = nil
    end
    hubcap_usage_cache = nil

    local response = {
        service = state.service,
        hasRyuuKey = utils.trim(tostring(state.ryuuKey or "")) ~= "",
        hasHubcapKey = utils.trim(tostring(state.hubcapKey or "")) ~= ""
    }
    if state.service == "hubcap" and response.hasHubcapKey then
        local usage_ok, usage_or_error = fetch_hubcap_usage(state.hubcapKey, true)
        if usage_ok then
            response.hubcapUsage = usage_or_error
        else
            response.validationError = tostring(usage_or_error)
        end
    end
    return reply(true, "Настройки сервиса сохранены", response)
end

function set_card_mode(mode)
    local clean = tostring(mode or ""):lower()
    if clean ~= "full" and clean ~= "button" and clean ~= "crocodile" then
        return reply(false, "Неизвестный режим карточки")
    end
    local state = load_state()
    state.cardMode = clean
    local ok, err = save_state(state)
    if not ok then
        return reply(false, "Не удалось сохранить настройку: " .. tostring(err))
    end
    return reply(true, "Режим карточки изменён", {
        cardMode = state.cardMode
    })
end

function remove_manifest(appid)
    local clean = tostring(appid or ""):match("^(%d+)$")
    if not clean then
        return reply(false, "Некорректный AppID")
    end
    local steam_path = millennium.steam_path()
    if not steam_path or steam_path == "" then
        return reply(false, "Millennium не смог определить папку Steam")
    end

    local state = load_state()
    local latest = latest_history(state, clean)
    local installed_game = state.installedGames[clean]
    local game_name = installed_game and tostring(installed_game.name or "")
        or (latest and tostring(latest.name or "") or ("AppID " .. clean))
    local stplugin = fs.join(steam_path, "config", "lua")
    local depotcache = fs.join(steam_path, "depotcache")
    local lua_paths = {
        fs.join(stplugin, clean .. ".lua"),
        fs.join(stplugin, clean .. ".lua.disabled")
    }
    local depot_ids = {}
    local removed = 0
    local failures = {}

    for _, path in ipairs(lua_paths) do
        if fs.is_file(path) then
            local content = read_binary(path) or ""
            for depot_id in content:gmatch("addappid%s*%(%s*(%d+)") do
                depot_ids[depot_id] = true
            end
            local ok, err = fs.remove(path)
            if ok then
                removed = removed + 1
            elseif err then
                table.insert(failures, fs.filename(path) .. ": " .. tostring(err))
            end
        end
    end

    local recorded_manifests = installed_game and installed_game.manifestFiles
        or (latest and latest.manifestFiles)
    if type(recorded_manifests) == "table" then
        for _, name in ipairs(recorded_manifests) do
            local path = fs.join(depotcache, tostring(name))
            if fs.is_file(path) then
                local ok, err = fs.remove(path)
                if ok then
                    removed = removed + 1
                elseif err then
                    table.insert(failures, tostring(name) .. ": " .. tostring(err))
                end
            end
        end
    end

    if fs.is_directory(depotcache) then
        local entries = fs.list(depotcache)
        if entries then
            for _, entry in ipairs(entries) do
                if entry.is_file then
                    local depot_id = tostring(entry.name):match("^(%d+)_.*%.manifest$")
                    if depot_id and depot_ids[depot_id] then
                        local ok, err = fs.remove(entry.path)
                        if ok then
                            removed = removed + 1
                        elseif err then
                            table.insert(failures, tostring(entry.name) .. ": " .. tostring(err))
                        end
                    end
                end
            end
        end
    end

    state.installedVersions[clean] = nil
    state.installedGames[clean] = nil
    local cache_dir = manifest_cache_directory(clean)
    if cache_dir and fs.is_directory(cache_dir) then
        local cache_ok, cache_err = fs.remove_all(cache_dir)
        if cache_ok == nil and cache_err then
            table.insert(failures, "резервная копия: " .. tostring(cache_err))
        end
    end
    save_state(state)
    version_cache[clean] = nil

    if #failures > 0 then
        local message = "Часть файлов не удалена: " .. table.concat(failures, "; ")
        add_history(clean, game_name, "error", message)
        return reply(false, message)
    end
    local message = removed > 0 and ("Удалено файлов: " .. tostring(removed)) or "Файлы игры уже отсутствуют"
    add_history(clean, game_name, "removed", message)
    return reply(true, message, {
        removedCount = removed
    })
end

function clear_history()
    local state = load_state()
    state.history = {}
    local ok, err = save_state(state)
    if not ok then
        return reply(false, "Не удалось очистить историю: " .. tostring(err))
    end
    return reply(true, "История очищена")
end

function restart_steam()
    if not is_windows() then
        return reply(false, "Перезапуск поддерживается только на Windows")
    end
    local steam_path = millennium.steam_path()
    if not steam_path or steam_path == "" then
        return reply(false, "Millennium не смог определить папку Steam")
    end
    local steam_exe = fs.join(steam_path, "steam.exe")
    if not fs.is_file(steam_exe) then
        return reply(false, "steam.exe не найден")
    end

    local temp_root = utils.getenv("TEMP") or utils.getenv("TMP")
    if not temp_root or temp_root == "" then
        return reply(false, "Не удалось определить временную папку Windows")
    end
    local token = tostring(os.time()) .. "-" .. utils.uuid()
    local script_path = fs.join(temp_root, "crocodile-manifest-restart-" .. token .. ".ps1")
    local launcher_path = fs.join(temp_root, "crocodile-manifest-launcher-" .. token .. ".ps1")
    local success_marker = fs.join(temp_root, "crocodile-manifest-launcher-" .. token .. ".ok")
    local script = "$ErrorActionPreference = 'SilentlyContinue'\n" ..
        "$steam = " .. powershell_quote(steam_exe) .. "\n" ..
        "$steamDir = " .. powershell_quote(steam_path) .. "\n" ..
        "$launcher = " .. powershell_quote(launcher_path) .. "\n" ..
        "$marker = " .. powershell_quote(success_marker) .. "\n" ..
        "try {\n" ..
        "  & $steam -shutdown | Out-Null\n" ..
        "  $deadline = (Get-Date).AddSeconds(15)\n" ..
        "  while ((Get-Process steam -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) {\n" ..
        "    Start-Sleep -Milliseconds 500\n" ..
        "  }\n" ..
        "  Get-Process steamwebhelper -ErrorAction SilentlyContinue | Stop-Process -Force\n" ..
        "  Get-Process steam -ErrorAction SilentlyContinue | Stop-Process -Force\n" ..
        "  Start-Sleep -Seconds 2\n" ..
        "  Start-Process -FilePath $steam -WorkingDirectory $steamDir\n" ..
        "} finally {\n" ..
        "  Start-Sleep -Seconds 1\n" ..
        "  Remove-Item -LiteralPath $launcher, $marker, $MyInvocation.MyCommand.Path -Force\n" ..
        "}\n"
    local ok, err = write_binary(script_path, script)
    if not ok then
        return reply(false, "Не удалось подготовить перезапуск: " .. tostring(err))
    end

    local child_command = 'powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' ..
        script_path:gsub('"', '""') .. '"'
    local launcher = "$ErrorActionPreference = 'Stop'\n" ..
        "$commandLine = " .. powershell_quote(child_command) .. "\n" ..
        "$result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $commandLine }\n" ..
        "if ([int]$result.ReturnValue -ne 0) { throw \"Win32_Process.Create: $($result.ReturnValue)\" }\n" ..
        "Set-Content -LiteralPath " .. powershell_quote(success_marker) .. " -Value 'ok' -NoNewline\n"
    local launcher_ok, launcher_err = write_binary(launcher_path, launcher)
    if not launcher_ok then
        fs.remove_all(script_path)
        return reply(false, "Не удалось подготовить скрытый перезапуск: " .. tostring(launcher_err))
    end
    local started, start_err = run_powershell(launcher_path, success_marker)
    if not started then
        fs.remove_all(script_path)
        fs.remove_all(launcher_path)
        fs.remove_all(success_marker)
        return reply(false, "Не удалось запустить перезапуск Steam: " .. tostring(start_err))
    end
    return reply(true, "Steam перезапускается")
end

local function on_load()
    local state = load_state()
    local changed = false
    for _, entry in ipairs(state.history) do
        if entry.pendingRestart == true then
            entry.pendingRestart = false
            changed = true
        end
    end
    if changed then
        save_state(state)
    end
    local steam_path = millennium.steam_path()
    if steam_path and steam_path ~= "" then
        for appid, installed_game in pairs(state.installedGames) do
            local latest = latest_history(state, tostring(appid))
            local manifest_files = recorded_manifest_files(steam_path, tostring(appid), installed_game, latest)
            sync_manifest_cache(steam_path, tostring(appid), manifest_files)
        end
    end
    logger:info("Crocodile Manifest loaded")
    millennium.ready()
end

local function on_unload()
    logger:info("Crocodile Manifest unloaded")
end

return {
    on_load = on_load,
    on_unload = on_unload
}
