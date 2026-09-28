local probe=...
local scene=true
assert(probe:Guard(function()return scene end,{},"scene_changed"))
assert(probe:Async(5))
local callback=assert(probe:Callback(function()probe:Finish("invalid scene entered")end))
local timer=C_Timer.NewTimer(1,function()scene=false;callback()end)
assert(probe:OnCleanup(function()timer:Cancel()end))
