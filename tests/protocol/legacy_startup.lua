-- Historical optical-engine fixtures provide their own startup adapter; the
-- current bootstrap is independently exercised by tests/addon/t_bootstrap.lua.
local ns,loader=...
loader:SetScript('OnEvent',function(self,event,name)
    if event=='ADDON_LOADED' and name~='Lychee Dev' then return end
    self:UnregisterAllEvents();self:SetScript('OnEvent',nil)
    local identity,reason=ns.Platform.ObserveBuild()
    if not identity then ns.Startup.ready,ns.Startup.reason=false,reason;return end
    local loaded,failure=ns.Persistence.Load()
    if not loaded then ns.Startup.ready,ns.Startup.reason=false,failure;return end
    ns.Startup.ready,ns.Startup.identity=true,identity
    local registered,registrationFailure=ns.Controls.Register()
    if not registered then ns.Startup.commandFailure=registrationFailure end
    if registered then
        local resumed,resumeFailure=ns.Reentry.Start(loader)
        if not resumed then ns.Startup.resumeFailure=resumeFailure end
    end
end)
