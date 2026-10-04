local Env = ...
local ns = Env.LoadWorkbench()
ns.Compat.MonotonicSeconds = function() return 0 end
local timers = {}
C_Timer.NewTimer = function(seconds, callback)
    local timer = {callback=callback}
    function timer:Cancel() self.cancelled=true end
    timers[#timers+1]=timer
    return timer
end
Env.LoadAddon("Bridge/ProbeExecution.lua", ns)
local callbacks, weak = {}, setmetatable({}, {__mode="v"})
local completed = 0
for i=1,140 do
    local resource = {payload=string.rep("x",8192)}
    weak[i]=resource
    local callback
    local api=ns.ProbeExecution.Run(function(p)
        assert(p:Async(10))
        callback=assert(p:Callback(function() return resource.payload end))
        assert(p:OnCleanup(function() assert(resource.payload) end))
    end,10,function(ok,value,meta)
        completed=completed+1
        assert(not ok and value=="probe_cancelled" and meta.cancelled and meta.resourcesReleased)
    end)
    assert(api:RequestCancel())
    assert(not api:RequestCancel(),"duplicate cancellation settled twice")
    callbacks[i]=callback -- Model an outside event source retaining its wrapper.
end
assert(completed==140)
collectgarbage("collect")
collectgarbage("collect")
for i=1,140 do
    assert(weak[i]==nil,"inactive callback retained probe environment "..i)
    local ok,reason=callbacks[i]()
    assert(not ok and reason=="probe_callback_inactive")
end
-- A probe failure with this spelling must not be misclassified as host cancel.
ns.ProbeExecution.Run(function(p) p:Fail("probe_cancelled") end,10,function(ok,_,meta)
    assert(not ok and not meta.cancelled)
end)
print("probe cancellation and 140 retained callback wrappers release environments")
