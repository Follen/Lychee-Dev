-- Executor sandbox: print/dump capture, prefix normalization, errors as
-- data, the 44 KB output bound and exact /run global semantics.
local Env, client, root = ...

local ns = Env.LoadWorkbench()
local L = ns.L

local originalPrint = print
local originalDump = dump

-- Combat refusal at entry.
Env.SetInCombat(true)
local blocked, blockedMessage = ns.Execute("return true")
Env.SetInCombat(false)
assert(blocked == false and blockedMessage == L.COMBAT_BLOCKED,
    "Lua execution was allowed in combat")

-- Empty input after normalization is an error message, not a run.
local emptyOk, emptyMessage, emptyCode = ns.Execute("   ")
assert(emptyOk == false and emptyMessage == L.ENTER_LUA and emptyCode == "",
    "empty input was not rejected")

-- Slash prefixes are stripped case-insensitively and the normalized code is
-- returned alongside the result.
local succeeded, result, normalized, valueTree, storedTree =
    ns.Execute('/run print("hello", 7); return { b = 2, a = 1 }, nil, "ok"')
assert(succeeded, result)
assert(normalized:sub(1, 5) == "print", "slash prefix was not removed")
assert(result:find("hello  7", 1, true), "print output was not captured")
assert(result:find('["a"] = 1', 1, true), "table was not serialized")
assert(result:find("[2] = nil", 1, true), "nil return value was lost")
assert(result:find('[3] = "ok"', 1, true), "multiple return values were lost")

local _, _, scriptNormalized = ns.Execute('/ScRiPt print(1)')
assert(scriptNormalized == "print(1)", "/script prefix was not stripped case-insensitively")

-- Return values build a live tree and a stored snapshot without live refs.
assert(valueTree and #valueTree.roots == 3, "structured return values were not captured")
assert(valueTree.roots[1].kind == "table", "table return was not represented as a tree")
assert(valueTree.roots[2].kind == "nil", "nil return was not represented in the tree")
assert(storedTree and #storedTree.roots == 3, "stored return tree was not created")
assert(storedTree.roots[1].source == nil and storedTree.roots[1].parent == nil,
    "stored tree retained runtime object references")

-- The captured print/dump never leak into the global environment.
assert(print == originalPrint, "captured print leaked into the global environment")
assert(dump == originalDump, "captured dump leaked into the global environment")

-- dump() serializes and returns its value in the captured output.
local dumpOk, dumpResult = ns.Execute('dump({ answer = 42 }); return 1')
assert(dumpOk and dumpResult:find('["answer"] = 42', 1, true),
    "dump() did not capture a bounded serialization")

-- Global writes behave exactly like /run.
_G.LycheeDevTestGlobal = nil
local writeOk, writeResult = ns.Execute("LycheeDevTestGlobal = 42")
assert(writeOk, writeResult)
assert(_G.LycheeDevTestGlobal == 42, "execution environment did not match /run global writes")
_G.LycheeDevTestGlobal = nil

-- Compile errors are returned as data.
local compileOk, compileResult = ns.Execute("this is not valid lua")
assert(compileOk == false and compileResult:find(L.COMPILE_ERROR, 1, true),
    "compile errors were not captured")

-- Runtime errors are appended to the captured output as data.
local runtimeOk, runtimeResult = ns.Execute('print("before"); error("expected failure")')
assert(runtimeOk == false and runtimeResult:find(L.RUNTIME_ERROR, 1, true)
        and runtimeResult:find("before", 1, true),
    "runtime errors were not captured")

-- The hard 44000-byte output cap bounds long prints.
local floodOk, floodResult = ns.Execute('print(string.rep("x", 50000))')
assert(floodOk, floodResult)
assert(#floodResult < 45000 and floodResult:find("<output truncated>", 1, true),
    "runtime output was not bounded")

-- The cap also bounds empty-print floods.
local emptyFloodOk, emptyFloodResult = ns.Execute("for index = 1, 50000 do print() end")
assert(emptyFloodOk, emptyFloodResult)
assert(#emptyFloodResult < 45000 and emptyFloodResult:find("<output truncated>", 1, true),
    "empty output bypassed the limit")

-- Runs without any output report completion explicitly.
local silentOk, silentResult, _, silentTree = ns.Execute("local ignored = 1")
assert(silentOk and silentResult == L.COMPLETED_NO_OUTPUT and silentTree == nil,
    "silent runs did not report completion")

print("execute ok")
