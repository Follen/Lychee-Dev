local ADDON_NAME, ns = ...

ns.Release = "3.2.0"
ns.Startup = { ready = false, reason = "addon_not_loaded" }
_G.LycheeDevInternal = ns

-- This small bootstrap loads before the workbench. It retains diagnostic
-- publication and manual reload even when a later module cannot start.
local clock = ns.Compat.MonotonicSeconds() or 0
local boot = string.format("%08x%04x%04x%04x%04x%08x", math.floor(clock),
    math.random(0,65535), math.random(0,65535), math.random(0,65535), math.random(0,65535), 1)
local zero = string.rep("0",32)
local phase, failure, sequence = "loading", "", 0
local publication = {schema="Lycheedev mailbox protocol v1",release=ns.Release,runtime=boot}
ns.BootstrapMailbox = publication
local function publish()
    sequence = sequence + 1
    local bindings, bindingFailure = ns.ReceiverBindings.Current()
    local value = {schema="Lycheedev mailbox protocol v1",bootstrap=boot,release=ns.Release,
        phase=phase,reason=failure,bindings=bindings or {wake="",reload=""},
        bindingFailure=bindingFailure or ""}
    local payload = ns.CaptureWriter.Encode(value,4096)
    if payload then publication.bootstrap=ns.MemoryProtocol.Encode(boot,boot,zero,6,1,sequence,payload) end
end
ns.Bootstrap = { Publish = publish }
local function reload()
    local ready, reason = ns.Compat.CanReload()
    if not ready then return nil, reason end
    return ns.Compat.ReloadUI()
end
local function wake()
    if not ns.Startup.ready or not ns.SlotRuntime then return nil,ns.Startup.reason or "startup_failed" end
    return ns.SlotRuntime.Wake()
end
local reported={}
local loader=CreateFrame("Frame")
loader:RegisterEvent("ADDON_LOADED")
loader:RegisterEvent("PLAYER_LOGIN")
loader:SetScript("OnEvent",function(self,event,name)
    if event=="ADDON_LOADED" and name~=ADDON_NAME then return end
    local bound,bindingFailure=ns.ReceiverBindings.Register({wake=wake,reload=reload})
    ns.Startup.receiverFailure=not bound and bindingFailure or nil
    local ok,started,reason
    if ns.StartRuntime then ok,started,reason=pcall(ns.StartRuntime)
    else ok,started,reason=true,nil,"startup_module_unavailable" end
    if not ok then started,reason=nil,"startup_exception" end
    if started then phase,failure="ready",""
    else
        phase,failure="failed",reason or "startup_failed"
        ns.Startup.ready,ns.Startup.reason=false,failure
        if not reported[failure] then print("Lychee Dev: "..failure);reported[failure]=true end
    end
    publish()
    if started or event=="PLAYER_LOGIN" then
        self:UnregisterAllEvents();self:SetScript("OnEvent",nil)
    end
end)
publish()
