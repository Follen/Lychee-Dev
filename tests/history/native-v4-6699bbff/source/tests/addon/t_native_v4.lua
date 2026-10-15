-- Candidate v4 protocol suites are additional cases; v3 historical suites remain intact.
local Env,client,addonRoot=...
local oldArg=arg
arg={addonRoot.."/.."}
assert(loadfile(addonRoot.."/../protocol/native-v4-test.lua"))()
assert(loadfile(addonRoot.."/../protocol/native-v4-routing-test.lua"))()
assert(loadfile(addonRoot.."/../protocol/native-v4-cost-test.lua"))()
arg=oldArg
assert(Env.framesCreated==0,"v4 protocol load created disabled frames")
