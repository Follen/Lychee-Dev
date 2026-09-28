local probe = ...
assert(probe:Async(75))
local timer = C_Timer.NewTimer(60, assert(probe:Callback(function()
    probe:Finish({ marker = "observation-survives-reload", inputReleased = not LycheeDevInternal.SlotRuntime.IsActive() })
end)))
assert(probe:OnCleanup(function() timer:Cancel() end))
