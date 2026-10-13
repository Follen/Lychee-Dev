-- Actual candidate singleton with fixture OS/API observations and real LoD files.
local root,addon=assert(arg[1]),assert(arg[2])
math.randomseed(tonumber((arg[3]or"00000001"):sub(1,8),16)%2147483647)
dofile(root.."/tests/lib/wow_globals.lua")
local Env=dofile(root.."/tests/addon/env.lua");Env.Init("retail",addon)
local ns=Env.LoadWorkbench();ns.Release="3.1.2";_G.LycheeDevInternal=ns
LycheeToolkitDB={schema=1,options={bridgeEnabled=false}};LycheeToolkitBridgeDB={schema=1,reports={}}
assert(ns.Persistence.Load())
local now=100
GetTime=function()return now end;IsPlayerInWorld=function()return true end
local actor="actor"
ns.Platform={ObserveBuild=function()return{build="fixture",product="retail"}end,ObserveActor=function()return{guid=actor,character="Tester",realm="Realm"}end,ObserveInputState=function()return true end}
local callbacks
ns.ReceiverBindings={Current=function()return{wake="ALT-CTRL-F12",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end,Register=function(value)callbacks=value;return true end}
ns.StartupBeacon={Stop=function()end};ns.ActivityView={Receiving=function()end,Begin=function()end,Collecting=function()end,Finish=function()end}
C_Timer.NewTimer=function()return{Cancel=function()end}end
local loaded,loads={},0
C_AddOns.LoadAddOn=function(name)
    loads=loads+1
    if loaded[name]then return true end
    loaded[name]=true
    assert(loadfile(addon.."/../"..name.."/Payload.lua"))()
    assert(loadfile(addon.."/../"..name.."/Loader.lua"))()
    return true
end
ns.Compat.GetAddOnMetadata=function(name,key)
    local file=assert(io.open(addon.."/../"..name.."/"..name..".toc","r"))
    local value
    for line in file:lines()do local field,text=line:match("^## ([^:]+): (.*)$");if field==key then value=text end end
    file:close();return value
end
Env.LoadAddon("Modules/AutomationHistory.lua",ns)
Env.LoadAddon("Modules/Diagnostics.lua",ns)
for _,p in ipairs({"CaptureWriter","NativeWire","NativeProtocol","NativeRuntime","InputProtection","ProbeExecution","InputSignal","NativeInputState","NativeAdapter"})do Env.LoadAddon("Bridge/"..p..".lua",ns)end
assert(ns.NativeRuntime.Register());assert(ns.NativeRuntime.Start())
local function hex(s)return(s:gsub(".",function(c)return string.format("%02x",string.byte(c))end))end
local function map(t)local keys={}for k in pairs(t or{})do keys[#keys+1]=k end;table.sort(keys);local items={}for _,k in ipairs(keys)do items[#items+1]=string.format('"%s":"%s"',k,hex(t[k]))end;return"{"..table.concat(items,",").."}"end
local function emit()
    local box=ns.Mailbox
    io.write('{"identity":"',hex(box.identity),'","input":"',hex(box.input or""),'","receipts":',map(box.receipts),',"bodies":',map(box.bodies),',"loads":',loads,'}\n');io.flush()
end
emit()
for line in io.lines()do
    if line=="BUGS_PRESENT"then
        BugGrabber={version="fixture-provider",GetSessionId=function()return 4 end,GetDB=function()return{
            {message="old",stack="old-stack",locals="",time=1,session=1,counter=1},
            {message="older 荔枝",stack="bounded",locals="",time=2,session=3,counter=2},
            {message="newest",stack=string.rep("s",90000),time=3,session=4,counter=3}
        }end};emit()
    elseif line=="BUGS_ABSENT"then BugGrabber=nil;emit()
    elseif line=="BUGS_ERROR"then BugGrabber={GetDB=function()error("provider fixture failure")end};emit()
    elseif line=="ACTIVATE"then assert(callbacks.wake());emit()
    elseif line=="SAMPLE"then now=now+1;ns.InputState.Refresh();emit()
    elseif line=="RELOAD"then
        ns.InputState.Stop();loaded={}
        for _,p in ipairs({"NativeRuntime","InputProtection","ProbeExecution","InputSignal","NativeInputState","NativeAdapter"})do Env.LoadAddon("Bridge/"..p..".lua",ns)end
        assert(ns.NativeRuntime.Register());assert(ns.NativeRuntime.Start());emit()
    elseif line=="OFF"then assert(ns.Controls==nil);LycheeToolkitDB.options.bridgeEnabled=false;ns.InputState.Stop();ns.NativeRuntime.Close();emit()
    elseif line=="ACTOR"then actor="foreign-actor";ns.InputState.Refresh();emit()
    else
        local kind=line:match("^(%w+)|")
        if kind=="OPEN"or kind=="EXECUTE"then callbacks.wake()
        elseif kind=="SETTLE"then callbacks.submit()
        elseif kind=="CLOSE"then callbacks.close()
        else error("fixture command invalid")end
        emit()
    end
end
