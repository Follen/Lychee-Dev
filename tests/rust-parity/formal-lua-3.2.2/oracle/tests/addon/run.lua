-- Standalone runner for one workbench test file under one client profile:
--   lua run.lua <t_file.lua> [client] [addon_root]
-- Later agents drop new t_*.lua files next to this runner; the Go test globs
-- them automatically.

local scriptPath = arg[0] or "run.lua"
local scriptDir = string.match(scriptPath, "^(.*)[/\\][^/\\]*$") or "."

local testFile = assert(arg[1], "usage: lua run.lua <t_file.lua> [client] [addon_root]")
if not string.find(testFile, "[/\\]") then
    testFile = scriptDir .. "/" .. testFile
end
local client = arg[2] or os.getenv("LYCHEEDEV_TEST_CLIENT") or "retail"
local addonRoot = arg[3] or (scriptDir .. "/../../addon")

-- Client globals (strmatch and friends) must exist before any addon source or
-- vendored library is loaded; env.lua installs the rest of the WoW surface.
dofile(scriptDir .. "/../lib/wow_globals.lua")

local Env = dofile(scriptDir .. "/env.lua")
Env.Init(client, addonRoot)

local chunk, loadError = loadfile(testFile)
assert(chunk, tostring(loadError))
-- Suites come in two shapes: harness suites take (Env, client, addonRoot) as
-- varargs, self-contained suites read arg[1] as the addon root (their
-- documented `lua t_x.lua [path/to/addon]` usage). Present both.
arg = { [0] = testFile, [1] = addonRoot, [2] = client }
chunk(Env, client, addonRoot)

io.write("PASS " .. testFile .. " [" .. client .. "]\n")
