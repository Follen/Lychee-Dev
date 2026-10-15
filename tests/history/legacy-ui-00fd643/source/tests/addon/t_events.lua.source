-- Event catalog and monitor tests. Ported from the legacy EventCatalogTests
-- and EventMonitorTests scenarios with the 2.0 module names: per-client
-- catalog counts and payloads, Find, search ranking and limits, ALL alias and
-- selection swap semantics, and the bounded monitor ring/driver behavior.

local createdFrames = 0
local eventDriver
local driverScript
local clock = 100
local secretValue

local function NewDriver()
    local frame = {
        events = {},
        scripts = {},
    }

    function frame:SetScript(name, callback)
        self.scripts[name] = callback
    end

    function frame:RegisterEvent(eventName)
        if eventName == "UNKNOWN_EVENT" then
            return
        end
        self.events[eventName] = true
    end

    function frame:RegisterAllEvents()
        self.allEvents = true
    end

    function frame:IsEventRegistered(eventName)
        return self.events[eventName] and true or false
    end

    function frame:UnregisterAllEvents()
        wipe(self.events)
        self.allEvents = nil
    end

    return frame
end

function CreateFrame()
    createdFrames = createdFrames + 1
    local frame = NewDriver()
    -- The monitor's event driver is the first frame it creates; later frames
    -- (for example the lazy combat-shutdown driver from Core/Safety.lua) must
    -- not replace the captured event channel.
    if not eventDriver then
        eventDriver = frame
        driverScript = frame.scripts
    end
    return frame
end

function wipe(target)
    for key in pairs(target) do
        target[key] = nil
    end
end

function GetTime()
    clock = clock + 0.1
    return clock
end

function issecretvalue(value)
    return value == secretValue
end

local function LoadFile(relativePath)
    local prefixes = { "addon/", "../../addon/", "../addon/" }
    for index = 1, #prefixes do
        local chunk = loadfile(prefixes[index] .. relativePath)
        if chunk then
            return chunk
        end
    end
    error("cannot load addon file: " .. relativePath)
end

local function LoadAddonFile(relativePath, namespace)
    return LoadFile(relativePath)("Lychee Dev", namespace)
end

-- ---------------------------------------------------------------------------
-- Per-client catalogs: counts, sorted names, payload signatures, presence.
-- ---------------------------------------------------------------------------
local clients = {
    { id = "retail", file = "Modules/Events/CatalogData_Mainline.lua", count = 1782, bnConnected = "suppressNotification", delve = true, arena = false, externalUrl = true },
    { id = "classic", file = "Modules/Events/CatalogData_Mists.lua", count = 1483, bnConnected = "", delve = false, arena = true, externalUrl = false },
    { id = "titan", file = "Modules/Events/CatalogData_Titan.lua", count = 1486, bnConnected = "", delve = false, arena = true, externalUrl = true },
    { id = "forever", file = "Modules/Events/CatalogData_Forever.lua", count = 1802, bnConnected = "suppressNotification", delve = true, arena = false, externalUrl = true },
}

local catalogNamespaces = {}
for index = 1, #clients do
    local client = clients[index]
    local namespace = {}
    LoadAddonFile("Core/Locale.lua", namespace)
    LoadAddonFile("Core/Locale_enUS.lua", namespace)
    LoadAddonFile(client.file, namespace)
    -- The unified catalog selects its per-product data field by ns.Client.
    namespace.Client = client.id
    LoadAddonFile("Modules/Events/Catalog.lua", namespace)
    catalogNamespaces[index] = namespace

    local catalog = namespace.EventCatalog
    assert(createdFrames == 0, client.id .. " event catalog created a frame while loading")
    assert(catalog.GetCount() == client.count,
        client.id .. " official event catalog count changed: " .. tostring(catalog.GetCount()))

    local previousName
    for entryIndex = 1, catalog.GetCount() do
        local eventName = catalog.Get(entryIndex)
        assert(type(eventName) == "string" and eventName ~= "",
            client.id .. " catalog contains an empty event name")
        assert(not previousName or previousName < eventName,
            client.id .. " catalog is not sorted by literal event name")
        previousName = eventName
    end

    local targetIndex = assert(catalog.Find("PLAYER_TARGET_CHANGED"),
        client.id .. " PLAYER_TARGET_CHANGED is missing")
    local targetName, targetSignature = catalog.Get(targetIndex)
    assert(targetName == "PLAYER_TARGET_CHANGED" and targetSignature == "",
        client.id .. " PLAYER_TARGET_CHANGED payload is incorrect")

    local healthIndex = assert(catalog.Find("UNIT_HEALTH"), client.id .. " UNIT_HEALTH is missing")
    local _, healthSignature = catalog.Get(healthIndex)
    assert(healthSignature == "unitTarget", client.id .. " UNIT_HEALTH payload is incorrect")

    local equipmentIndex = assert(catalog.Find("PLAYER_EQUIPMENT_CHANGED"),
        client.id .. " PLAYER_EQUIPMENT_CHANGED is missing")
    local _, equipmentSignature = catalog.Get(equipmentIndex)
    assert(equipmentSignature == "equipmentSlot, hasCurrent",
        client.id .. " PLAYER_EQUIPMENT_CHANGED payload is incorrect")

    local bnConnectedIndex = assert(catalog.Find("BN_CONNECTED"), client.id .. " BN_CONNECTED is missing")
    local _, bnConnectedSignature = catalog.Get(bnConnectedIndex)
    assert(bnConnectedSignature == client.bnConnected,
        client.id .. " BN_CONNECTED payload is incorrect")

    if client.delve then
        assert(catalog.Find("ACTIVE_DELVE_DATA_UPDATE"), client.id .. " delve event is missing")
    else
        assert(not catalog.Find("ACTIVE_DELVE_DATA_UPDATE"),
            client.id .. " catalog contains a Retail delve event")
    end
    if client.arena then
        assert(catalog.Find("ARENA_TEAM_UPDATE"), client.id .. " arena event is missing")
    else
        assert(not catalog.Find("ARENA_TEAM_UPDATE"),
            client.id .. " catalog contains a removed arena event")
    end
    if client.externalUrl then
        assert(catalog.Find("EXTERNAL_EVENT_LAUNCH_URL_FAILED"),
            client.id .. " external URL event is missing")
    else
        assert(not catalog.Find("EXTERNAL_EVENT_LAUNCH_URL_FAILED"),
            client.id .. " catalog contains a foreign external URL event")
    end
end

-- ---------------------------------------------------------------------------
-- Find, search ranking and limit, ALL alias. Run the behavior suite against
-- every client catalog.
-- ---------------------------------------------------------------------------
for index = 1, #clients do
    local client = clients[index]
    local catalog = catalogNamespaces[index].EventCatalog

    assert(catalog.Find("unit_health") == catalog.Find("UNIT_HEALTH"),
        client.id .. " Find did not normalize case")
    assert(catalog.Find("") == nil and catalog.Find("NO_SUCH_EVENT_NAME") == nil,
        client.id .. " Find returned a hit for an unknown event")
    assert(catalog.Find("all") == 0, client.id .. " ALL alias did not resolve to index 0")
    local allName = catalog.Get(0)
    assert(allName == "ALL", client.id .. " ALL pseudo-event has the wrong name")

    local healthIndex = catalog.Find("UNIT_HEALTH")
    local exactResults = catalog.Search("unit_health", 8)
    assert(exactResults[1] == healthIndex, client.id .. " exact event match was not ranked first")

    local unitResults = catalog.Search("unit", catalog.GetCount())
    local previousRank = 0
    local sawPrefix
    local sawSubstring
    for resultIndex = 1, #unitResults do
        local eventName = catalog.Get(unitResults[resultIndex])
        local rank = eventName:sub(1, 4) == "UNIT" and 2 or 3
        assert(rank >= previousRank,
            client.id .. " prefix and substring search ranks are out of order")
        previousRank = rank
        sawPrefix = sawPrefix or rank == 2
        sawSubstring = sawSubstring or rank == 3
    end
    assert(sawPrefix and sawSubstring,
        client.id .. " UNIT search did not exercise both prefix and substring matches")
    assert(#catalog.Search("UNIT", 8) == 8, client.id .. " search result limit was not enforced")

    local allResults = catalog.Search("all", 8)
    assert(allResults[1] == 0, client.id .. " ALL was not ranked as an exact search result")

    -- Selection: unlimited count (64+), dedupe, remove, ALL swap semantics.
    local selection = catalog.CreateSelection()
    for selectionIndex = 1, 64 do
        local succeeded, errorMessage = selection:Add(selectionIndex)
        assert(succeeded, errorMessage)
    end
    assert(selection:GetCount() == 64, client.id .. " selection retained an old event-count limit")
    assert(createdFrames == 0, client.id .. " selecting events created a frame")

    local firstSelected = selection:Get(1)
    assert(selection:Contains(firstSelected), client.id .. " selected event lookup failed")
    local duplicate, duplicateError = selection:Add(catalog.Find(firstSelected))
    assert(not duplicate and duplicateError == catalogNamespaces[index].L.EVENT_ALREADY_SELECTED,
        client.id .. " duplicate selection was not rejected with EVENT_ALREADY_SELECTED")
    assert(selection:Remove(firstSelected), client.id .. " selected event could not be removed")
    assert(selection:GetCount() == 63 and not selection:Contains(firstSelected),
        client.id .. " event removal left stale selection state")

    local succeeded, errorMessage = selection:AddName("player_target_changed")
    assert(succeeded, errorMessage)
    assert(selection:Contains("PLAYER_TARGET_CHANGED"),
        client.id .. " exact event names were not normalized when selected")
    assert(createdFrames == 0, client.id .. " editing the selection registered runtime machinery")

    succeeded = selection:AddName("\229\133\168\233\131\168")
    assert(succeeded and selection:GetCount() == 1,
        client.id .. " ALL did not replace individual selections")
    assert(selection:Get(1) == "ALL", client.id .. " ALL selection stored the wrong event")
    succeeded = selection:AddName("UNIT_HEALTH")
    assert(succeeded and selection:GetCount() == 1,
        client.id .. " individual selection did not replace ALL")
    assert(selection:Get(1) == "UNIT_HEALTH", client.id .. " selection did not leave ALL mode")

    local unknown, unknownError = selection:AddName("NO_SUCH_EVENT_NAME")
    assert(not unknown and unknownError == catalogNamespaces[index].L.NO_DOCUMENTED_EVENT,
        client.id .. " unknown event selection did not report NO_DOCUMENTED_EVENT")
end

print("event catalog tests passed")

-- ---------------------------------------------------------------------------
-- Monitor: lazy driver, bounded ring, argument formatting, atomic failure.
-- ---------------------------------------------------------------------------
local monitorNamespace = {}
LoadAddonFile("Core/Locale.lua", monitorNamespace)
LoadAddonFile("Core/Locale_enUS.lua", monitorNamespace)
LoadAddonFile("Core/Safety.lua", monitorNamespace)
LoadAddonFile("Modules/Events/Monitor.lua", monitorNamespace)
local monitor = monitorNamespace.EventMonitor

assert(createdFrames == 0, "event monitor created a frame before Start")

local callbackCount = 0
local succeeded, errorMessage = monitor.Start({ "PLAYER_TARGET_CHANGED", "PLAYER_REGEN_ENABLED" }, function()
    callbackCount = callbackCount + 1
end)
assert(succeeded, errorMessage)
assert(createdFrames >= 1, "event monitor did not lazily create its driver")
local framesAfterStart = createdFrames
assert(monitor.IsRunning(), "event monitor did not enter running state")
assert(monitor.GetActiveEventCount() == 2, "event monitor did not register every event")

driverScript.OnEvent(eventDriver, "PLAYER_TARGET_CHANGED", "player", 7, true)
assert(callbackCount == 1, "event monitor did not notify its listener")
assert(monitor.GetCount() == 1, "event record was not stored")
local record = monitor.GetRecord(1)
assert(record.event == "PLAYER_TARGET_CHANGED", "wrong event was stored")
assert(record.elapsed >= 0 and record.elapsed < 1, "event elapsed time was not relative to Start")
assert(record.summary:find('"player"', 1, true), "event arguments were not formatted")

-- Argument caps: MAX_ARGUMENTS 16 + "<N more arguments>", MAX_ARGUMENT_BYTES
-- 256 with a "..." suffix, tables as <table>, secrets as <secret>.
local manyArguments = {}
for index = 1, 18 do
    manyArguments[index] = index
end
driverScript.OnEvent(eventDriver, "UNIT_HEALTH", unpack(manyArguments))
local manyRecord = monitor.GetRecord(1)
assert(#manyRecord.arguments == 17, "argument count cap was not applied")
assert(manyRecord.arguments[17] == "<2 more arguments>", "overflow arguments marker is wrong")

driverScript.OnEvent(eventDriver, "UNIT_HEALTH", string.rep("x", 300))
local longArgument = monitor.GetRecord(1).arguments[1]
assert(#longArgument == 259 and longArgument:sub(-3) == "...",
    "argument byte cap was not applied")

driverScript.OnEvent(eventDriver, "UNIT_HEALTH", { alpha = 1 })
assert(monitor.GetRecord(1).arguments[1] == "<table>", "table argument was not capped")

secretValue = {}
driverScript.OnEvent(eventDriver, "UNIT_HEALTH", secretValue)
assert(monitor.GetRecord(1).arguments[1] == "<secret>", "secret argument was not masked")
secretValue = nil

-- 500-ring: firing 510 keeps the newest record.
for index = 1, 510 do
    driverScript.OnEvent(eventDriver, "PLAYER_REGEN_ENABLED", index)
end
assert(monitor.GetCount() == 500, "event ring buffer was not bounded")
assert(monitor.GetRecord(1).summary == "510", "event ring buffer lost newest record")

monitor.Stop()
assert(not monitor.IsRunning(), "event monitor did not stop")
assert(next(eventDriver.events) == nil, "event monitor left events registered")

monitor.Clear()
assert(monitor.GetCount() == 0, "event monitor did not clear records")

-- Unknown event: atomic rejection leaves nothing registered.
succeeded = monitor.Start({ "PLAYER_TARGET_CHANGED", "UNKNOWN_EVENT" })
assert(not succeeded, "event monitor accepted an unavailable event")
assert(next(eventDriver.events) == nil, "failed Start left events registered")

succeeded = monitor.Start({ "bad name" })
assert(not succeeded, "event monitor accepted an invalid event name")

succeeded = monitor.Start({ "ALL", "UNIT_HEALTH" })
assert(succeeded, errorMessage)
assert(eventDriver.allEvents, "ALL mode did not call RegisterAllEvents")
assert(monitor.IsMonitoringAllEvents(), "ALL mode state was not exposed")
assert(monitor.GetActiveEventCount() == 1, "ALL mode was counted as individual events")
monitor.Stop()
assert(not eventDriver.allEvents, "ALL mode remained registered after Stop")
assert(not monitor.IsMonitoringAllEvents(), "ALL mode state remained active after Stop")

assert(createdFrames == framesAfterStart, "restarting the monitor created extra frames")

print("event monitor tests passed")
