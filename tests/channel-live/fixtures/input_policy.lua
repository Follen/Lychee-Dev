local probe=...
assert(probe:Async(18))
assert(probe:ProtectInput(12))
local result={protected=probe:IsInputProtected(),during=LycheeDevInternal.ActivityView.Current()}
local release=C_Timer.NewTimer(8,assert(probe:Callback(function()
    assert(probe:ReleaseInput())
    result.released=not LycheeDevInternal.InputProtection.IsActive()
    result.after=LycheeDevInternal.ActivityView.Current()
end)))
local finish=C_Timer.NewTimer(14,assert(probe:Callback(function()probe:Finish(result)end)))
assert(probe:OnCleanup(function()release:Cancel();finish:Cancel()end))
