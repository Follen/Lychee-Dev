local root=assert(arg[1])
assert(loadfile(arg[2]))()
local profiles={
    {version="12.1.0",interface=120100,product="retail"},
    {version="5.5.4",interface=50504,product="classic"},
    {version="3.80.2",interface=38002,product="titan"},
    {version="1.60.1",interface=16001,product="forever"},
}
local outputs={}

for _,profile in ipairs(profiles) do
    local actor={character="Paladin",realm="Realm",guid="Player-1-123"}
    local nonce,reloadNonce=string.rep("a",32),string.rep("b",32)
    local frames,shown,reloads={},nil,0
    LycheeToolkitDB=nil
    SlashCmdList,SLASH_LYCHEETOOLKIT1={},nil
    issecretvalue=function() return false end
    GetBuildInfo=function() return profile.version,"12345","date",profile.interface end
    UnitFullName=function() return actor.character,actor.realm end
    UnitGUID=function() return actor.guid end
    IsLoggedIn=function() return true end
    InCombatLockdown=function() return false end
    GetCurrentKeyBoardFocus=function() return nil end
    GetLocale=function() return "enUS" end
    time=function() return 123456 end
    BugGrabber={version="fixture",GetSessionId=function() return 7 end,GetDB=function()
        return {
            {message="old",stack="old stack",locals="old locals",source="Old",time=1,session=6,counter=1},
            {message="first",stack="first stack",locals="first locals",source="One",time=2,session=7,counter=2},
            {message="second",stack="second stack",locals="second locals",source="Two",time=3,session=7,counter=3},
        }
    end}
    C_UI={Reload=function() reloads=reloads+1 end}
    CreateFrame=function(kind)
        assert(kind=="Frame")
        local frame={events={}}
        function frame:RegisterEvent(event) self.events[event]=true end
        function frame:UnregisterAllEvents() self.events={} end
        function frame:SetScript(event,callback) assert(event=="OnEvent");self.callback=callback end
        frames[#frames+1]=frame
        return frame
    end

    local function loadRuntime()
        SlashCmdList,SLASH_LYCHEETOOLKIT1={},nil
        local ns={}
        local toc=assert(io.open(root.."/Lychee Dev.toc","r"))
        for line in toc:lines() do
            line=line:gsub("\r",""):gsub("\\","/")
            if line~="" and line:sub(1,1)~="#" then
                assert(loadfile(root.."/"..line))("Lychee Dev",ns)
            end
        end
        toc:close()
        ns.ReceiptView={Hide=function() shown=nil end,Show=function(value) shown=value;return true end,
            ShowIdentity=function(value) shown=value;return true end}
        local loader=frames[#frames]
        loader.callback(loader,"ADDON_LOADED","Lychee Dev")
        return ns,loader
    end

    local ns,loader=loadRuntime()
    assert(ns.Controls.Handle("bridge on"))
    assert(ns.Controls.Handle("bridge bind "..nonce))

    -- Standalone reload is independently correlated and carries no report.
    assert(ns.Controls.Handle("bridge refresh "..reloadNonce)=="reload_requested")
    assert(reloads==1 and LycheeToolkitDB.reentry.standalone==true)
    ns,loader=loadRuntime()
    loader.callback(loader,"PLAYER_ENTERING_WORLD",false,true)
    assert(ns.Session.Current()==nil and shown==nil,"standalone ready before loading ended")
    loader.callback(loader,"LOADING_SCREEN_DISABLED")
    assert(ns.Session.Current().runtimeEpoch==2 and shown and LycheeToolkitDB.reentry==nil)
    outputs[#outputs+1]=shown

    -- Built-in bugs is a bounded report operation: report, one persistence
    -- reload, then explicit acknowledgement with no cleanup reload.
    local reported=assert(ns.Controls.Handle("bridge bugs BUGS-A 2"))
    local stored,body=ns.ReportStore.Read("BUGS-A")
    assert(stored==reported and body:find('"requestType":"bugs"',1,true))
    assert(body:find('"requestedCount":2',1,true) and body:find('"returnedCount":2',1,true))
    assert(ns.Controls.Handle("bridge flush BUGS-A "..reloadNonce)=="reload_requested")
    assert(reloads==2 and LycheeToolkitDB.reentry.builtin==true)
    ns,loader=loadRuntime()
    loader.callback(loader,"LOADING_SCREEN_DISABLED")
    assert(ns.Session.Current()==nil,"builtin ready before world entry")
    loader.callback(loader,"PLAYER_ENTERING_WORLD",false,true)
    assert(ns.Session.Current().runtimeEpoch==3 and shown and ns.ReportStore.Read("BUGS-A")==reported)
    outputs[#outputs+1]=shown
    local sequence=assert(reported:match('"sequence":(%d+)'))
    local acknowledged=assert(ns.Controls.Handle("bridge bugs-ack BUGS-A "..sequence))
    assert(reloads==2 and ns.ReportStore.Read("BUGS-A")==nil)
    local repeated=assert(ns.Controls.Handle("bridge bugs-ack BUGS-A "..sequence))
    assert(repeated~=acknowledged and reloads==2, "bugs ACK recovery must only reissue its receipt")
    assert(ns.Controls.Handle("bridge bugs-ack BUGS-A "..(tonumber(sequence)+1))==nil,
        "bugs ACK recovery accepted another report sequence")
    outputs[#outputs+1]=acknowledged
end

local ns={}
assert(loadfile(root.."/Bridge/CaptureWriter.lua"))("Lychee Dev",ns)
io.write(assert(ns.CaptureWriter.Encode(outputs)))
