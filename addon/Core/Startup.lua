local _, ns = ...

-- The bootstrap owns the loader and bindings; business startup owns persistence
-- and the workbench. A failed initialization never removes bootstrap diagnostics.
local initialized=false
ns.StartRuntime=function()
    if not initialized then
        local identity,reason=ns.Platform.ObserveBuild()
        if not identity then return nil,reason end
        local loaded,failure=ns.Persistence.Load()
        if not loaded then return nil,failure end
        ns.Startup.ready,ns.Startup.reason,ns.Startup.identity=true,nil,identity
        local registered,registrationFailure=ns.Controls.Register()
        ns.Startup.commandFailure=not registered and registrationFailure or nil
        initialized=true
    end
    local ok,reason=ns.SlotRuntime.Start()
    if ok then ns.Startup.ready,ns.Startup.reason=true,nil end
    return ok,reason
end
