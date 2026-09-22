-- Locale parity: zhCN source, zhTW inherits it, every other locale gets the
-- ASCII English overlay, and both tables stay in key/placeholder lockstep.
local Env, client, root = ...

local function LoadLocale(locale)
    Env.SetLocale(locale)
    local ns = Env.NewNamespace()
    Env.LoadAddon("Core/Locale.lua", ns)
    Env.LoadAddon("Core/Locale_enUS.lua", ns)
    return ns.L
end

local function CountKeys(values)
    local count = 0
    for _ in pairs(values) do
        count = count + 1
    end
    return count
end

local function CollectPlaceholders(value)
    local placeholders = {}
    for placeholder in value:gmatch("%%[%d%$%+%-%.# ]*[cdeEfgGiouXxqs]") do
        placeholders[#placeholders + 1] = placeholder
    end
    return table.concat(placeholders, "|")
end

local chinese = LoadLocale("zhCN")
local traditionalChinese = LoadLocale("zhTW")
local english = LoadLocale("enUS")
local britishEnglish = LoadLocale("enGB")
local internationalFallback = LoadLocale("deDE")

local chineseKeyCount = CountKeys(chinese)
local englishKeyCount = CountKeys(english)
assert(chineseKeyCount > 0, "the Chinese locale table is empty")
assert(chineseKeyCount == englishKeyCount,
    "locale key counts differ: zhCN=" .. chineseKeyCount .. " enUS=" .. englishKeyCount)

for key, chineseValue in pairs(chinese) do
    local englishValue = english[key]
    assert(type(englishValue) == "string" and englishValue ~= "", "missing English locale key: " .. key)
    assert(not englishValue:find("[\128-\255]"), "English locale value is not ASCII: " .. key)
    assert(CollectPlaceholders(chineseValue) == CollectPlaceholders(englishValue),
        "format placeholders differ for locale key: " .. key)
    assert(traditionalChinese[key] == chineseValue, "zhTW must preserve the Chinese fallback: " .. key)
    assert(britishEnglish[key] == englishValue, "enGB must use the English locale: " .. key)
    assert(internationalFallback[key] == englishValue, "non-Chinese locales must use English: " .. key)
end

for key in pairs(english) do
    assert(chinese[key] ~= nil, "unexpected English locale key: " .. key)
end

-- "运行" in the source table; the assertion stays ASCII like the test source.
assert(chinese.TAB_RUNNER ~= english.TAB_RUNNER and chinese.TAB_RUNNER:find("[\128-\255]"),
    "zhCN locale was unexpectedly overridden")
assert(english.TAB_RUNNER == "Run", "enUS runner label is incorrect")
assert(english.ADDON_TITLE:find("Lychee", 1, true), "English addon title is incorrect")
assert(english.ABOUT_CLIENT_VALUE:find("Retail 12.1", 1, true), "English client summary lacks Retail")
assert(english.ABOUT_CLIENT_VALUE:find("Classic 5.5.4", 1, true), "English client summary lacks Classic")
assert(english.ABOUT_CLIENT_VALUE:find("Titan 3.80.2", 1, true), "English client summary lacks Titan")
assert(english.ABOUT_CLIENT_VALUE:find("Forever 1.60.1", 1, true), "English client summary lacks Forever")
assert(chinese.ABOUT_CLIENT_VALUE:find("12.1", 1, true)
        and chinese.ABOUT_CLIENT_VALUE:find("5.5.4", 1, true)
        and chinese.ABOUT_CLIENT_VALUE:find("3.80.2", 1, true)
        and chinese.ABOUT_CLIENT_VALUE:find("1.60.1", 1, true),
    "Chinese client summary lacks a supported client")

-- Every legacy key still exists under its legacy name.
local legacyKeys = {
    "ADDON_TITLE", "TAB_RUNNER", "TAB_OBJECTS", "TAB_EVENTS", "TAB_TRACE",
    "TAB_DIAGNOSTICS", "TAB_EXPORTS", "TAB_AUTOMATION", "TAB_ABOUT",
    "HISTORY", "NO_HISTORY", "EMPTY_INPUT", "LUA_INPUT", "RUN", "CLEAR_INPUT",
    "RESULT", "TEXT", "TREE", "SELECT_RESULT", "SAVE_TO_DISK", "CLEAR_HISTORY",
    "ENTER_LUA", "COMPILE_ERROR", "RUNTIME_ERROR", "COMPLETED_NO_OUTPUT",
    "COMBAT_BLOCKED", "CLOSE", "YES", "NO", "ON", "OFF", "UNKNOWN",
    "UNKNOWN_TIME", "NOT_AVAILABLE", "READY", "COMPLETED", "FAILED",
    "FIND_EVENT", "CATALOG_COUNT", "EVENT_SEARCH_HINT", "START_MONITORING",
    "STOP_MONITORING", "MONITORING_EVENTS", "ALL_EVENTS", "PAYLOAD_PREFIX",
    "FUNCTION_TRACE", "CALL_RECORDS", "CALL_ARGUMENTS", "TRACE_PATH",
    "START_TRACE", "STOP_TRACE", "EXPORT_KIND_FUNCTION_TRACE",
    "DIAGNOSTICS", "AGENT_REPORT", "REPORT_STACK", "REPORT_LOCALS",
    "EXPORT_KIND_ERROR_LOG", "EXPORT_SAVED_TITLE", "EXPORT_TICKET",
    "EXPORT_TICKET_HELP", "SAVE_TO_DISK", "RELOAD_NOW", "RELOAD_LATER",
    "CONFIRM_CLEAR_CACHE", "EXPORT_STATUS_PENDING", "EXPORT_STATUS_SAVED",
    "EXPORT_KIND_RUN_RESULT", "EXPORT_KIND_OBJECT_SNAPSHOT",
    "EXPORT_KIND_OBJECT_NODE", "EXPORT_KIND_EVENT_LOG",
    "SECRET_VALUE_BLOCKED", "MATCH_COUNT", "NODE_TEXT_TITLE",
    "TEXT_LIMIT_REACHED", "OBJECT_SNAPSHOT", "PICKER_PROMPT", "SEARCH",
    "AUTO_USAGE", "AUTO_KIND_BUG", "ABOUT_TITLE", "ABOUT_DESCRIPTION",
    "ABOUT_CLIENT", "ABOUT_COMMAND", "ABOUT_DEPENDENCY", "ABOUT_GITHUB",
    "ABOUT_SAFETY", "TREE_TABLE_MORE", "FRAME_OVERVIEW", "FRAME_PARENT",
}
for index = 1, #legacyKeys do
    local key = legacyKeys[index]
    assert(type(chinese[key]) == "string" and chinese[key] ~= "", "legacy locale key lost: " .. key)
    assert(type(english[key]) == "string" and english[key] ~= "", "legacy English key lost: " .. key)
end

print("locale parity ok")
