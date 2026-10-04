local root = assert(arg[1])
assert(loadfile(arg[2]))()
local frozen=setmetatable({}, {__mode="k"})
table.freeze=function(row)frozen[row]=true;return row end
table.isfrozen=function(row)return frozen[row]==true end
local frames={}
LycheeToolkitDB={schema=1,options={bridgeEnabled=false}}
LycheeToolkitBridgeDB=nil
CreateFrame=function()
    local frame={events={},scripts={}}
    function frame:RegisterEvent(name)self.events[name]=true end
    function frame:UnregisterAllEvents()self.events={}end
    function frame:SetScript(name,fn)self.scripts[name]=fn end
    function frame:Hide()self.hidden=true end
    frames[#frames+1]=frame;return frame
end
SlashCmdList={}
GetBuildInfo=function()return "12.1.0","69933","date",120100 end
GetTime=function()return 100 end
UnitFullName=function()return "Paladin","Realm" end
local actorObservations=0
UnitGUID=function()actorObservations=actorObservations+1;return "Player-1-1" end
IsLoggedIn=function()return true end
IsPlayerInWorld=function()return true end
InCombatLockdown=function()return false end
GetCurrentKeyBoardFocus=function()return nil end
local ns={}
local toc=assert(io.open(root.."/Lychee Dev.toc","r"))
for line in toc:lines() do
    line=line:gsub("\r","")
    if line~="" and line:sub(1,1)~="#" then
        assert(loadfile(root.."/"..line:gsub("\\","/")))("Lychee Dev",ns)
    end
end
toc:close()
assert(#frames==1 and frames[1].scripts.OnEvent)
frames[1].scripts.OnEvent(frames[1],"ADDON_LOADED","Lychee Dev")
assert(ns.Startup.ready and not ns.Mailbox and #frames==1)
collectgarbage("collect")
local baseline=collectgarbage("count")
assert(ns.DuplexRuntime.Start())
collectgarbage("collect")
assert(collectgarbage("count")-baseline<1 and #frames==1,"disabled Start retained work")
assert(ns.DuplexProtocol.MaxSource==1048576 and ns.DuplexProtocol.MaxFrames==1,
    "physical arena changes must retain 1 MiB logical request capacity")
LycheeToolkitDB.options.bridgeEnabled=true
assert(ns.DuplexRuntime.Enable())
local ticker=frames[#frames]
assert(#frames==2 and ticker.scripts.OnUpdate)
local runtime=ns.Mailbox.runtime
collectgarbage("collect")
local enabled=collectgarbage("count")
collectgarbage("stop")
local began=os.clock()
local observationsBefore=actorObservations
for _=1,100 do ticker.scripts.OnUpdate(ticker,0.05) end
assert(actorObservations-observationsBefore==5,"idle heartbeat repeated actor observation")
local idleGarbage=collectgarbage("count")-enabled
local idleCPU=os.clock()-began
collectgarbage("restart")
collectgarbage("collect")
local retained=collectgarbage("count")-enabled
local peak=collectgarbage("count")
for _=1,1000 do
    ticker.scripts.OnUpdate(ticker,0.05)
    peak=math.max(peak,collectgarbage("count"))
end
collectgarbage("collect")
local final=collectgarbage("count")-enabled
print(string.format("mailbox v1 Runtime memory: enabled arena delta %.2f KiB; idle 5s garbage %.2f KiB, CPU %.3fs; post-GC retained %.2f KiB; idle 50s peak %.2f KiB, final retained %.2f KiB",
    enabled-baseline,idleGarbage,idleCPU,retained,peak-baseline,final))
assert(ns.Mailbox.runtime==runtime and not ns.DuplexRuntime.Snapshot().protocol.quarantined,
    "idle observation must not rotate or quarantine arena")
assert(enabled-baseline<9216,"fixed mailbox arena exceeds 9 MiB stock-Lua budget")
assert(idleGarbage<128,"idle Runtime allocated more than 128 KiB in five seconds")
assert(retained<32 and final<32,"idle Runtime retained unbounded garbage after collection")
-- Warm every row's negative observation cache. These stable but invalid
-- headers must never execute; their numeric shadows still exercise the
-- maximum fixed cache cost that a cold all-zero arena would miss.
local rows={ns.Mailbox.inbox.command,ns.Mailbox.inbox.stop}
for _,row in ipairs(rows) do row[75]=2;row[77]=2 end
ticker.scripts.OnUpdate(ticker,0.05)
collectgarbage("collect")
local warm=collectgarbage("count")
assert(warm-baseline<9216,"transport marker caches exceed fixed arena budget")
collectgarbage("stop")
for _=1,100 do ticker.scripts.OnUpdate(ticker,0.05) end
local warmGarbage=collectgarbage("count")-warm
collectgarbage("restart");collectgarbage("collect")
assert(warmGarbage<128,"unchanged warm lanes repeatedly allocated decode work")
assert(not ns.DuplexRuntime.Snapshot().protocol.request,"invalid warm publication started work")
print(string.format("mailbox warm cache: all-lane retained delta %.2f KiB; idle 5s garbage %.2f KiB",warm-baseline,warmGarbage))
assert(ns.DuplexRuntime.Disable())
assert(ticker.scripts.OnUpdate==nil,"disabled Runtime retained polling hook")
