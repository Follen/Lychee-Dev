local probe=...
assert(probe:Async(35))
local timer=C_Timer.NewTimer(25,probe:Callback(function()
    probe:Finish({marker="native-async", inputReleased=not LycheeDevInternal.SlotRuntime.IsActive()})
end))
assert(probe:OnCleanup(function() timer:Cancel() end))
