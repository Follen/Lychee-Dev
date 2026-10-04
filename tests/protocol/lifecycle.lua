local root = assert(arg[1])
assert(loadfile(arg[2]))()
local secret = {}; issecretvalue=function(v)return rawequal(v,secret)end
local profiles={
    {version="12.1.0",interface=120100,product="retail"},
    {version="5.5.4",interface=50504,product="classic"},
    {version="3.80.2",interface=38002,product="titan"},
}
local function boot(profile,enabled,delayed)
    local frames={}
    local loggedIn=true
    LycheeToolkitDB={schema=1,options={bridgeEnabled=enabled}}
    LycheeToolkitBridgeDB=nil
    CreateFrame=function(kind)
        local f={events={},scripts={}}
        function f:RegisterEvent(name)self.events[name]=true end
        function f:UnregisterAllEvents()self.events={}end
        function f:SetScript(name,fn)self.scripts[name]=fn end
        function f:Hide()self.hidden=true end
        frames[#frames+1]=f;return f
    end
    SlashCmdList={};SLASH_LYCHEETOOLKIT1=nil
    GetBuildInfo=function()return profile.version,"12345","date",profile.interface end
    GetTime=function()return 12.5 end
    UnitFullName=function()return "Paladin","Realm" end
    UnitGUID=function()return "Player-1-12345" end
    IsLoggedIn=function()return loggedIn end;IsPlayerInWorld=function()return true end;InCombatLockdown=function()return false end
    GetCurrentKeyBoardFocus=function()return nil end
    local ns={}
    local f=assert(io.open(root.."/Lychee Dev.toc","r"))
    for line in f:lines() do line=line:gsub("\r","");if line~="" and line:sub(1,1)~="#" then
        assert(loadfile(root.."/"..line:gsub("\\","/")))("Lychee Dev",ns)
    end end;f:close()
    assert(#frames==1 and frames[1].events.ADDON_LOADED)
    local loader=frames[1];loader.scripts.OnEvent(loader,"ADDON_LOADED",delayed and "different internal name" or "Lychee Dev")
    if delayed then
        assert(not ns.Startup.ready and loader.events.PLAYER_LOGIN)
        assert(not ns.Startup.ready)
        loader.scripts.OnEvent(loader,"PLAYER_LOGIN")
    end
    assert(ns.Startup.ready and ns.Startup.identity.product==profile.product)
    if enabled then
        assert(ns.Mailbox and ns.Mailbox.schema=="lycheedev.mailbox.v1")
        assert(ns.DuplexRuntime.Snapshot().enabled and #frames==2)
        assert(ns.Mailbox.inbox.calibration[1]==0 and ns.Mailbox.inbox.calibration[6]==7654321)
        assert(ns.DuplexProtocol.FrameSlots==1 and #ns.Mailbox.inbox.request.frames==ns.DuplexProtocol.FrameSlots
            and #ns.Mailbox.inbox.request.frames[1]==1104)
        assert(ns.DuplexProtocol.MaxFrames==256 and ns.DuplexProtocol.MaxSource==1048576)
        assert(#ns.Mailbox.inbox.control.bindResume==336 and ns.Mailbox.sendbox.status:sub(1,8)=="LYCMSB01")
        local framesRef=ns.Mailbox.inbox.request.frames
        local weak=setmetatable({framesRef},{__mode="v"})
        ns.Mailbox=nil;collectgarbage("collect")
        assert(weak[1]==framesRef,"private owner did not root the arena")
        local removed=framesRef[1];local weakRow=setmetatable({removed},{__mode="v"})
        framesRef[1]=nil;removed=nil;collectgarbage("collect")
        assert(weakRow[1]~=nil,"private arena roots did not retain a removed public frame row")
        framesRef[1]=weakRow[1]
        for _=1,4 do frames[2].scripts.OnUpdate(frames[2],0.016) end
        assert(ns.Mailbox and ns.Mailbox.inbox.request.frames==framesRef,"runtime failed to republish rooted mailbox")
        loggedIn=false
        for _=1,70 do frames[2].scripts.OnUpdate(frames[2],0.016) end
        local status=assert(ns.DuplexProtocol.ParseSendbox(ns.Mailbox.sendbox.status))
        assert(status:find('"actorReady":false',1,true),"logout did not revoke live actor readiness: "..status)
        loggedIn=true
        for _=1,70 do frames[2].scripts.OnUpdate(frames[2],0.016) end
        status=assert(ns.DuplexProtocol.ParseSendbox(ns.Mailbox.sendbox.status))
        assert(status:find('"actorReady":true',1,true),"login did not restore live actor readiness")
        assert(ns.Controls.Handle("disconnect"))
        assert(not ns.DuplexRuntime.Snapshot().enabled and frames[2].scripts.OnUpdate==nil)
    else
        assert(ns.Mailbox==nil and ns.DuplexRuntime.Snapshot().runtime==nil)
        assert(#frames==1 and frames[1].scripts.OnUpdate==nil,"disabled feature allocated an active frame")
    end
    return ns,frames
end

for _,profile in ipairs(profiles) do
    boot(profile,false,false)
    boot(profile,true,false)
end
boot(profiles[1],false,true) -- PLAYER_LOGIN retries an unsupported first observation.
local old=boot(profiles[1],true,false)
local oldRuntime=old.DuplexRuntime.Snapshot().runtime
local damaged,damagedFrames=boot(profiles[1],true,false)
LycheeToolkitDB.options.bridgeEnabled=true
assert(damaged.DuplexRuntime.Enable())
damaged.Mailbox.inbox.calibration[4]=secret
damagedFrames[#damagedFrames].scripts.OnUpdate(damagedFrames[#damagedFrames],0.05)
assert(damaged.DuplexRuntime.Snapshot().protocol.quarantined==true
    and damaged.DuplexRuntime.Snapshot().protocol.lastFailure=="duplex_repair_unavailable",
    "unbound damaged topology was not safely quarantined")
assert(damaged.Controls.Handle("disconnect"))
local fresh=boot(profiles[1],true,false)
assert(fresh.DuplexRuntime.Snapshot().runtime~=oldRuntime,"reload reused a runtime token")
print("lifecycle: mailbox v1 startup, opt-in runtime, GC root and reload passed")
