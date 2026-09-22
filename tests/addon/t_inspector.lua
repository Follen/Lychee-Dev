-- Value trees: bounded paging, lazy children, cycle and mutation markers,
-- secret handling, frame overview nodes and stored-tree caps.
local Env, client, root = ...

local ns = Env.LoadWorkbench()
local I = ns.Inspector
local L = ns.L

-- Paging: 450 entries load one bounded page of 200 first.
local largeTable = {}
for index = 1, 450 do
    largeTable["field" .. index] = index
end
local largeTree = I.CreateLiveTree({ n = 1, largeTable })
local largeRoot = largeTree.roots[1]
assert(largeRoot.loadedCount == 200 and #largeRoot.children == 201,
    "large tree did not load one bounded page initially")
assert(largeRoot.children[#largeRoot.children].kind == "load_more",
    "large tree did not expose the next page action")
assert(I.LoadMore(largeRoot) and largeRoot.loadedCount == 400 and largeRoot.hasMore,
    "large tree did not append its second page")
assert(I.LoadMore(largeRoot) and largeRoot.loadedCount == 450 and not largeRoot.hasMore
        and #largeRoot.children == 450,
    "large tree did not expose every entry after paging")
assert(string.find(largeRoot.value, "450", 1, true), "table summary lost its entry count")

-- Lazy children: nested tables stay unexpanded until requested.
local nestedTree = I.CreateLiveTree({ n = 1, { child = { value = 7 } } })
local nestedNode = nestedTree.roots[1].children[1]
assert(nestedNode.kind == "table" and not nestedNode.loaded and #nestedNode.children == 0,
    "nested table was eagerly inspected")
assert(I.LoadMore(nestedNode) and nestedNode.loaded and #nestedNode.children == 1,
    "nested table did not load when explicitly requested")

-- Cycles become marker nodes.
local treeCycle = {}
treeCycle.self = treeCycle
local cycleTree = I.CreateLiveTree({ n = 1, treeCycle })
assert(cycleTree.roots[1].children[1].kind == "marker"
        and cycleTree.roots[1].children[1].value == L.TREE_CYCLE,
    "tree cycle was not represented safely")

-- Mutation during iteration becomes a visible marker.
local mutating = {}
for index = 1, 300 do
    mutating["key" .. index] = index
end
local mutationTree = I.CreateLiveTree({ n = 1, mutating })
local mutationRoot = mutationTree.roots[1]
local cursorKey = mutationRoot.cursor
assert(cursorKey ~= nil, "tree paging did not keep a continuation cursor")
-- Removing the cursor key and rehashing the table makes next() reject the old
-- cursor, which is exactly how a mutating source surfaces mid-iteration.
mutating[cursorKey] = nil
for index = 1, 300 do
    mutating["fresh" .. index] = index
end
I.LoadMore(mutationRoot)
local lastChild = mutationRoot.children[#mutationRoot.children]
assert(lastChild.kind == "marker" and lastChild.value == L.TREE_TABLE_CHANGED,
    "mutation during iteration was not surfaced")

-- Secret keys are counted and skipped, secret values become markers.
local withSecretKeys = { visible = "yes", [Env.MakeSecret()] = "hidden" }
local secretTree = I.CreateLiveTree({ n = 1, withSecretKeys })
local secretRoot = secretTree.roots[1]
local secretMarker = secretRoot.children[#secretRoot.children]
assert(secretMarker.kind == "marker"
        and secretMarker.value == string.format(L.TREE_SECRET_KEYS, 1),
    "secret keys were not counted and skipped")
local secretValueTree = I.CreateLiveTree({ n = 1, Env.MakeSecret() })
assert(secretValueTree.roots[1].kind == "marker"
        and secretValueTree.roots[1].value == L.TREE_SECRET,
    "secret values leaked into the tree")

-- UI objects get a frame-overview node with lazy children/regions nodes.
local frame = CreateFrame("Frame", "InspectorTestFrame")
frame:SetSize(100, 50)
local childFrame = CreateFrame("Frame", nil, frame)
frame.GetChildren = function()
    return childFrame
end
local frameTree = I.CreateLiveTree({ n = 1, frame })
local frameRoot = frameTree.roots[1]
assert(frameRoot.kind == "table" and frameRoot.isUIObject, "UI object was not detected")
local overviewNode = frameRoot.children[1]
assert(overviewNode.label == L.FRAME_OVERVIEW and overviewNode.kind == "table",
    "UI object did not expose its overview node")
local overview = overviewNode.source
assert(overview[L.FRAME_OBJECT_TYPE] == "Frame" and overview[L.FRAME_NAME] == "InspectorTestFrame",
    "frame overview lost its identity fields")
local childrenNode = nil
for index = 1, #frameRoot.children do
    if frameRoot.children[index].label == L.FRAME_CHILDREN then
        childrenNode = frameRoot.children[index]
    end
end
assert(childrenNode and childrenNode.source[1] == childFrame,
    "UI object children node was not built")

-- Stored trees: no live references, capped depth/entries/value text.
local storedTree = I.CreateStoredTree({ n = 1, { player = { name = "Follen" } } })
local storedRoot = storedTree.roots[1]
assert(storedRoot.loaded and storedRoot.source == nil and storedRoot.parent == nil,
    "stored tree retained runtime object references")

local deepStored = {}
local deepCursor = deepStored
for index = 1, 12 do
    deepCursor.child = {}
    deepCursor = deepCursor.child
end
local deepStoredTree = I.CreateStoredTree({ n = 1, deepStored })
assert(deepStoredTree.truncated, "deep stored tree was not flagged truncated")

local wideStoredTree = I.CreateStoredTree({ n = 1, largeTable })
local wideRoot = wideStoredTree.roots[1]
assert(wideStoredTree.truncated and #wideRoot.children <= 200,
    "stored tree entry cap of 200 is not enforced")

local longText = string.rep("y", 2000)
local textStoredTree = I.CreateStoredTree({ n = 1, { value = longText } })
local textNode = textStoredTree.roots[1].children[1]
assert(#textNode.value == 512 + 3 and textNode.value:sub(-3) == "...",
    "stored tree value text cap of 512 bytes is not enforced")

local hugeStored = {}
for index = 1, 200 do
    local child = {}
    for inner = 1, 25 do
        child["field" .. inner] = inner
    end
    hugeStored["item" .. index] = child
end
local hugeStoredTree = I.CreateStoredTree({ n = 1, hugeStored })
assert(hugeStoredTree.truncated, "stored tree node cap of 4000 is not enforced")

-- Combat blocks tree paging.
Env.SetInCombat(true)
local ok, reason = I.LoadMore(largeRoot)
Env.SetInCombat(false)
assert(ok == false and reason == L.COMBAT_BLOCKED, "tree paging was allowed in combat")

print("inspector ok")
