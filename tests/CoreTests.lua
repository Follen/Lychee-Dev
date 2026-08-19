SlashCmdList = {}
local inCombat = false

function InCombatLockdown()
    return inCombat
end

function CreateFrame()
    return {
        RegisterEvent = function() end,
        SetScript = function() end,
    }
end

function wipe(target)
    for key in pairs(target) do
        target[key] = nil
    end
end

function time()
    return 1234567890
end

function issecretvalue()
    return false
end

local function LoadAddonFile(path, namespace)
    local chunk, loadError = loadfile(path)
    assert(chunk, loadError)
    return chunk("Lychee Dev", namespace)
end

local ns = {}
LoadAddonFile("Locale.lua", ns)
LoadAddonFile("Database.lua", ns)
LoadAddonFile("Serializer.lua", ns)
LoadAddonFile("Inspector.lua", ns)
LoadAddonFile("Safety.lua", ns)
LoadAddonFile("Core.lua", ns)

assert(SLASH_LYCHEEDEV1 == "/dev", "slash command was not renamed")
assert(type(SlashCmdList.LYCHEEDEV) == "function", "slash command was not registered")
assert(ns.db == nil, "database initialized before first use")

LycheeDevDB = nil
DumperDB = {
    schemaVersion = 1,
    history = {
        { code = "legacy code", result = "legacy result", succeeded = true },
        {
            code = "return { player = { name = 'Follen', level = 80 } }",
            result = '[1] = {\n  ["player"] = {\n    ["level"] = 80,\n    ["name"] = "Follen",\n  },\n}',
            succeeded = true,
        },
    },
}
local toggled = false
ns.ToggleWindow = function()
    toggled = true
end
SlashCmdList.LYCHEEDEV()
assert(type(ns.db) == "table", "database did not initialize on first use")
assert(LycheeDevDB == ns.db, "database was not moved to the Lychee Dev root")
assert(DumperDB == nil, "legacy database root was not cleared after migration")
assert(ns.db.history[1].code == "legacy code", "legacy history was not preserved")
assert(ns.db.schemaVersion == 5, "database schema was not upgraded")
assert(ns.db.history[2].tree and ns.db.history[2].tree.roots[1].children[1].label == "player",
    "legacy serialized history was not restored as a tree")
assert(toggled, "slash command did not toggle the window")

inCombat = true
toggled = false
SlashCmdList.LYCHEEDEV()
assert(not toggled, "slash command opened the window in combat")
local combatSucceeded, combatMessage = ns.Execute("return true")
assert(not combatSucceeded and combatMessage == ns.L.COMBAT_BLOCKED, "Lua execution was allowed in combat")
inCombat = false

local originalPrint = print
local originalDump = dump
local succeeded, result, normalized, valueTree, storedTree = ns.Execute('/run print("hello", 7); return { b = 2, a = 1 }, nil, "ok"')
assert(succeeded, result)
assert(print == originalPrint, "captured print leaked into the global environment")
assert(dump == originalDump, "captured dump leaked into the global environment")
assert(normalized:sub(1, 5) == "print", "slash prefix was not removed")
assert(result:find("hello  7", 1, true), "print output was not captured")
assert(result:find('["a"] = 1', 1, true), "table was not serialized")
assert(result:find("[2] = nil", 1, true), "nil return value was lost")
assert(result:find('[3] = "ok"', 1, true), "multiple return values were lost")
assert(valueTree and #valueTree.roots == 3, "structured return values were not captured")
assert(valueTree.roots[1].kind == "table", "table return was not represented as a tree")
assert(valueTree.roots[2].kind == "nil", "nil return was not represented in the tree")
assert(storedTree and #storedTree.roots == 3, "stored return tree was not created")
assert(storedTree.roots[1].source == nil and storedTree.roots[1].parent == nil,
    "stored tree retained runtime object references")

local largeTable = {}
for index = 1, 450 do
    largeTable["field" .. index] = index
end
local largeTree = ns.CreateValueTree({ n = 1, largeTable })
local largeRoot = largeTree.roots[1]
assert(largeRoot.loadedCount == 200 and #largeRoot.children == 201,
    "large tree did not load one bounded page initially")
assert(largeRoot.children[#largeRoot.children].kind == "load_more",
    "large tree did not expose the next page action")
ns.LoadMoreValueTreeNode(largeRoot)
assert(largeRoot.loadedCount == 400 and largeRoot.hasMore,
    "large tree did not append its second page")
ns.LoadMoreValueTreeNode(largeRoot)
assert(largeRoot.loadedCount == 450 and not largeRoot.hasMore and #largeRoot.children == 450,
    "large tree did not expose every entry after paging")

local nestedTable = { child = { value = 7 } }
local nestedTree = ns.CreateValueTree({ n = 1, nestedTable })
local nestedNode = nestedTree.roots[1].children[1]
assert(nestedNode.kind == "table" and not nestedNode.loaded and #nestedNode.children == 0,
    "nested table was eagerly inspected")
ns.LoadMoreValueTreeNode(nestedNode)
assert(nestedNode.loaded and #nestedNode.children == 1,
    "nested table did not load when explicitly requested")

local treeCycle = {}
treeCycle.self = treeCycle
local cycleTree = ns.CreateValueTree({ n = 1, treeCycle })
assert(cycleTree.roots[1].children[1].kind == "marker"
    and cycleTree.roots[1].children[1].value == ns.L.TREE_CYCLE,
    "tree cycle was not represented safely")
assert(not ns.L.TREE_CYCLE:find("node limit reached", 1, true)
    and not ns.L.TREE_CYCLE:find("more entries omitted", 1, true),
    "legacy tree omission markers are still present")

_G.LycheeDevTestGlobal = nil
succeeded, result = ns.Execute("LycheeDevTestGlobal = 42")
assert(succeeded, result)
assert(_G.LycheeDevTestGlobal == 42, "execution environment did not match /run global writes")
_G.LycheeDevTestGlobal = nil

succeeded, result = ns.Execute("this is not valid lua")
assert(not succeeded and result:find("编译错误：", 1, true), "compile errors were not captured")

succeeded, result = ns.Execute('error("expected failure")')
assert(not succeeded and result:find("运行错误：", 1, true), "runtime errors were not captured")

succeeded, result = ns.Execute('print(string.rep("x", 50000))')
assert(succeeded, result)
assert(#result < 45000 and result:find("<output truncated>", 1, true), "runtime output was not bounded")

succeeded, result = ns.Execute("for index = 1, 50000 do print() end")
assert(succeeded, result)
assert(#result < 45000 and result:find("<output truncated>", 1, true), "empty output bypassed the limit")

local cyclic = {}
cyclic.self = cyclic
assert(ns.Serialize(cyclic):find("<cycle>", 1, true), "cycles were not bounded")

local streamedValue = {}
for index = 1, 3000 do
    streamedValue["streamField" .. index] = string.rep("v", 16)
end
local stream = ns.CreateSerializationStream(streamedValue)
local firstChunk, firstFinished = stream:ReadChunk()
assert(#firstChunk <= 44000 and not firstFinished,
    "serialization stream did not stop after its first chunk")
local streamedParts = { firstChunk }
while not stream:IsFinished() do
    local chunk = stream:ReadChunk()
    streamedParts[#streamedParts + 1] = chunk
end
local streamedText = table.concat(streamedParts)
assert(streamedText:find('["streamField3000"]', 1, true),
    "serialization stream did not continue from its saved position")
assert(not stream:WasLimited(), "bounded stream unexpectedly hit its safety limit")

local secret = {}
issecretvalue = function(value)
    return value == secret
end
assert(ns.Serialize(secret) == "<secret>", "secret values were inspected")
issecretvalue = function()
    return false
end

local storedEntry = ns.AddHistory("stored tree", "result", true, storedTree)
assert(storedEntry.tree == storedTree and storedEntry.tree.roots[1].loaded,
    "history did not retain the stored return tree")

for index = 1, 35 do
    ns.AddHistory("code " .. index, "result " .. index, true)
end
assert(#ns.GetHistory() == 38, "history still has the old 30-entry limit")
assert(ns.GetHistory()[1].code == "code 35", "history is not newest-first")

for index = 36, 1200 do
    ns.AddHistory("code " .. index, "result " .. index, true)
end
assert(#ns.GetHistory() == 1024, "history storage budget was not enforced")
assert(ns.GetHistory()[1].code == "code 1200", "history budget removed the newest entry")

ns.ClearHistory()
assert(#ns.GetHistory() == 0, "history was not cleared")

print("Lychee Dev core tests passed")
