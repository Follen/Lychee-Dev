local function LoadLocale(locale)
    GetLocale = function()
        return locale
    end

    local ns = {}
    local baseChunk, baseError = loadfile("Core/Locale.lua")
    assert(baseChunk, baseError)
    baseChunk("Lychee Dev", ns)

    local englishChunk, englishError = loadfile("Core/Locale_enUS.lua")
    assert(englishChunk, englishError)
    englishChunk("Lychee Dev", ns)
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

assert(CountKeys(chinese) == 394, "unexpected Chinese locale key count")
assert(CountKeys(english) == 394, "unexpected English locale key count")

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

assert(chinese.TAB_RUNNER == "运行", "zhCN locale was unexpectedly overridden")
assert(english.TAB_RUNNER == "Run", "enUS runner label is incorrect")
assert(english.ADDON_TITLE:find("Lychee", 1, true), "English addon title is incorrect")
assert(english.ABOUT_CLIENT_VALUE:find("Retail 12.1", 1, true), "English client summary lacks Retail")
assert(english.ABOUT_CLIENT_VALUE:find("Classic 5.5.4", 1, true), "English client summary lacks Classic")
assert(english.ABOUT_CLIENT_VALUE:find("Titan 3.80.2", 1, true), "English client summary lacks Titan")

print("Locale tests passed")
