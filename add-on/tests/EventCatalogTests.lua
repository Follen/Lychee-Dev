local createdFrames = 0

function CreateFrame()
    createdFrames = createdFrames + 1
end

local function LoadAddonFile(path, namespace)
    local chunk, loadError = loadfile(path)
    assert(chunk, loadError)
    return chunk("Lychee Dev", namespace)
end

local ns = {}
local clientFiles = {
    retail = "Core/Clients/Mainline.lua",
    classic = "Core/Clients/Mists.lua",
    titan = "Core/Clients/Titan.lua",
}
local catalogFiles = {
    retail = "Modules/Events/CatalogData_Mainline.lua",
    classic = "Modules/Events/CatalogData_Mists.lua",
    titan = "Modules/Events/CatalogData_Titan.lua",
}
local expectedCounts = {
    retail = 1782,
    classic = 1483,
    titan = 1486,
}
local testClient = os.getenv("LYCHEE_TEST_CLIENT") or "retail"
LoadAddonFile(assert(clientFiles[testClient], "unknown test client: " .. testClient), ns)
LoadAddonFile("Core/Compatibility.lua", ns)
LoadAddonFile("Core/Locale.lua", ns)
LoadAddonFile("Core/Locale_enUS.lua", ns)
LoadAddonFile(assert(catalogFiles[testClient]), ns)
LoadAddonFile("Modules/Events/Catalog.lua", ns)

local catalog = ns.EventCatalog
assert(catalog.GetCount() == expectedCounts[testClient],
    testClient .. " official event catalog count changed")
assert(createdFrames == 0, "event catalog created a frame while loading")

local previousName
for index = 1, catalog.GetCount() do
    local eventName = catalog.Get(index)
    assert(type(eventName) == "string" and eventName ~= "", "catalog contains an empty event name")
    assert(not previousName or previousName < eventName, "catalog is not sorted by literal event name")
    previousName = eventName
end

local targetIndex = assert(catalog.Find("PLAYER_TARGET_CHANGED"), "PLAYER_TARGET_CHANGED is missing")
local targetName, targetSignature = catalog.Get(targetIndex)
assert(targetName == "PLAYER_TARGET_CHANGED" and targetSignature == "", "PLAYER_TARGET_CHANGED payload is incorrect")

local healthIndex = assert(catalog.Find("UNIT_HEALTH"), "UNIT_HEALTH is missing")
local _, healthSignature = catalog.Get(healthIndex)
assert(healthSignature == "unitTarget", "UNIT_HEALTH payload is incorrect")

local equipmentIndex = assert(catalog.Find("PLAYER_EQUIPMENT_CHANGED"), "PLAYER_EQUIPMENT_CHANGED is missing")
local _, equipmentSignature = catalog.Get(equipmentIndex)
assert(equipmentSignature == "equipmentSlot, hasCurrent", "PLAYER_EQUIPMENT_CHANGED payload is incorrect")

local bnConnectedIndex = assert(catalog.Find("BN_CONNECTED"), "BN_CONNECTED is missing")
local _, bnConnectedSignature = catalog.Get(bnConnectedIndex)
if testClient == "retail" then
    assert(catalog.Find("ACTIVE_DELVE_DATA_UPDATE"), "Retail-specific delve event is missing")
    assert(not catalog.Find("ARENA_TEAM_UPDATE"), "Retail catalog contains a Classic arena event")
    assert(catalog.Find("EXTERNAL_EVENT_LAUNCH_URL_FAILED"), "Retail external URL event is missing")
    assert(bnConnectedSignature == "suppressNotification", "Retail BN_CONNECTED payload is incorrect")
elseif testClient == "classic" then
    assert(not catalog.Find("ACTIVE_DELVE_DATA_UPDATE"), "Classic catalog contains a Retail delve event")
    assert(catalog.Find("ARENA_TEAM_UPDATE"), "Classic arena event is missing")
    assert(not catalog.Find("EXTERNAL_EVENT_LAUNCH_URL_FAILED"),
        "Classic catalog contains a Titan external URL event")
    assert(bnConnectedSignature == "", "Classic BN_CONNECTED payload is incorrect")
else
    assert(not catalog.Find("ACTIVE_DELVE_DATA_UPDATE"), "Titan catalog contains a Retail delve event")
    assert(catalog.Find("ARENA_TEAM_UPDATE"), "Titan arena event is missing")
    assert(catalog.Find("EXTERNAL_EVENT_LAUNCH_URL_FAILED"), "Titan external URL event is missing")
    assert(bnConnectedSignature == "", "Titan BN_CONNECTED payload is incorrect")
end

local exactResults = catalog.Search("unit_health", 8)
assert(exactResults[1] == healthIndex, "exact event match was not ranked first")

local unitResults = catalog.Search("unit", catalog.GetCount())
local previousRank = 0
local sawPrefix
local sawSubstring
for index = 1, #unitResults do
    local eventName = catalog.Get(unitResults[index])
    local rank = eventName:sub(1, 4) == "UNIT" and 2 or 3
    assert(rank >= previousRank, "prefix and substring search ranks are out of order")
    previousRank = rank
    sawPrefix = sawPrefix or rank == 2
    sawSubstring = sawSubstring or rank == 3
end
assert(sawPrefix and sawSubstring, "UNIT search did not exercise both prefix and substring matches")
assert(#catalog.Search("UNIT", 8) == 8, "search result limit was not enforced")

local selection = catalog.CreateSelection()
for index = 1, 64 do
    local succeeded, errorMessage = selection:Add(index)
    assert(succeeded, errorMessage)
end
assert(selection:GetCount() == 64, "selection retained an old event-count limit")
assert(createdFrames == 0, "selecting events created a frame")

local firstSelected = selection:Get(1)
assert(selection:Contains(firstSelected), "selected event lookup failed")
assert(selection:Remove(firstSelected), "selected event could not be removed")
assert(selection:GetCount() == 63 and not selection:Contains(firstSelected), "event removal left stale selection state")

local succeeded = selection:AddName("player_target_changed")
assert(succeeded, "exact event names were not normalized when selected")
assert(createdFrames == 0, "editing the selection registered runtime machinery")

succeeded = selection:AddName("全部")
assert(succeeded and selection:GetCount() == 1, "ALL did not replace individual selections")
assert(selection:Get(1) == "ALL", "ALL selection stored the wrong event")
succeeded = selection:AddName("UNIT_HEALTH")
assert(succeeded and selection:GetCount() == 1, "individual selection did not replace ALL")
assert(selection:Get(1) == "UNIT_HEALTH", "selection did not leave ALL mode")
local allResults = catalog.Search("all", 8)
assert(allResults[1] == 0, "ALL was not ranked as an exact search result")

print("Lychee Dev event catalog tests passed")
