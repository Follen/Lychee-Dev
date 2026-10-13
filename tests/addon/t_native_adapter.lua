local Env,client=...
local ns=Env.LoadWorkbench()
ns.Release="3.1.2"
local state={options={bridgeEnabled=false}}
local bridge={}
ns.Persistence={Current=function()return state end,Bridge=function()return bridge end,BridgeEnabled=function()return state.options.bridgeEnabled end}
GetTime=function()return 100 end
IsPlayerInWorld=function()return true end
ns.Platform={ObserveBuild=function()return{build="fixture",product=client}end,ObserveActor=function()return{guid="actor",character="Tester",realm="Realm"}end,ObserveInputState=function()return true end}
local callbacks
ns.ReceiverBindings={Current=function()return{wake="ALT-CTRL-F12",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end,Register=function(value)callbacks=value;return true end}
ns.AutomationHistory={Begin=function()end}
ns.StartupBeacon={Stop=function()end}
ns.ActivityView={Finish=function()end,Receiving=function()end}
C_Timer.NewTimer=function(_,fn)return{Cancel=function()end}end
local files,loaded,loads={}, {},0
C_AddOns.LoadAddOn=function(name)
    local index=tonumber(name:match("(%d+)$"));loads=loads+1
    if loaded[index]then return true end -- Real LoD semantics: never rerun.
    loaded[index]=true
    ns.NativeRuntime.Receive(index,files[index])
    return true
end
local metadataCalls=0
ns.Compat.GetAddOnMetadata=function(name,key)
    metadataCalls=metadataCalls+1
    if key=="Version"then return ns.Release end
    if key=="X-Lychee-Transport"then return"memory-slot-v4"end
    return tostring(tonumber(name:match("(%d+)$")))
end
local before=Env.framesCreated
for _,file in ipairs({"CaptureWriter","NativeWire","NativeProtocol","NativeRuntime","InputProtection","ProbeExecution","InputSignal","NativeInputState","NativeAdapter"})do Env.LoadAddon("Bridge/"..file..".lua",ns)end
assert(Env.framesCreated==before,"disabled candidate created a feature frame")
assert(ns.NativeRuntime.Start() and ns.Mailbox.identity and loads==0 and metadataCalls==0 and Env.framesCreated==before)
assert(ns.NativeRuntime.Register() and callbacks)
assert(callbacks.wake() and state.options.bridgeEnabled and loads==0,"activation loaded a business/bootstrap slot")
assert(metadataCalls==1500 and ns.Mailbox.schema=="lycheedev.mailbox.v2")
assert(ns.InputState.Snapshot():sub(1,8)=="LYCMEM07" and ns.InputState.Snapshot():find('"schema":"lycheedev.input.v4"',1,true))
local nonce=0
local owner=string.rep("a",32)
local epoch=string.rep("b",32)
local ticket=string.rep("c",32)
local function envelope(action,index,extra)
    nonce=nonce+1
    local identity=ns.NativeRuntime.Snapshot()
    local value={schema="lycheedev.slot.v4",index=index,runtime=identity.runtime,owner=owner,fence=1,sessionEpoch=epoch,nonce=string.format("%032x",nonce),ticket=ticket,requestSeq=1,digest="digest",guid="actor",build="fixture",challenge=identity.challenge,action=action}
    for key,x in pairs(extra or{})do value[key]=x end
    return value
end
files[1]=envelope("open",1,{lane=0,runtime=string.rep("f",32)})
files[5]=envelope("open",5,{lane=2})
assert(callbacks.wake())
assert(loads==3 and loaded[1] and loaded[3] and loaded[5] and not loaded[2] and not loaded[4] and not loaded[6])
assert(ns.NativeRuntime.Snapshot().nextSlot==141 and ns.NativeRuntime.Snapshot().owner==owner)
-- Private state does not read caller-edited Mailbox identity.
ns.Mailbox.identity="modified publication"
files[141]=envelope("execute",141,{code="return 7",budget=30,expires=200})
assert(callbacks.wake())
assert(ns.Mailbox.bodies[ticket] and ns.NativeRuntime.Snapshot().nextSlot==142)
for i=1,1000 do assert(not callbacks.wake())end
assert(loads==4,"late wake consumed control reserve")
local payload=assert(ns.CaptureWriter.Encode({ok=true,result=7,resourcesReleased=true,logs={}},524288))
files[142]=envelope("settle",142,{resultBytes=#payload,resultChecksum=ns.NativeWire.Checksum(payload)})
assert(callbacks.submit())
assert(ns.Mailbox.bodies[ticket]==nil and ns.NativeRuntime.Snapshot().nextSlot==143)
files[143]=envelope("close",143)
assert(callbacks.close())
local closeLoads=loads
assert(not callbacks.close() and loads==closeLoads)
-- Scanned foreign bind modules are Loaded, even though no private Receive ran.
-- Same runtime reconnect/changed lane must request reload, never reuse them.
files[1]=envelope("open",1,{lane=0,fence=2})
local ok,reason=callbacks.wake();assert(not ok and reason=="native_reload_required" and loads==closeLoads)
local snap=ns.NativeRuntime.Snapshot();snap.nextSlot=1;snap.owner="fake"
assert(ns.NativeRuntime.Snapshot().nextSlot==144 and ns.NativeRuntime.Snapshot().owner=="")
state.options.bridgeEnabled=false;ns.InputState.Stop();ns.NativeRuntime.Close()
assert(not ns.InputState.Snapshot() and ns.Mailbox.input:find('"activationOnly":true',1,true) and loads==closeLoads)
assert(callbacks.wake() and state.options.bridgeEnabled and loads==closeLoads,"explicit-off activation loaded a slot")
assert(not callbacks.wake() and loads==closeLoads,"closed runtime silently rebound after Enable")
assert(metadataCalls==1500+3*6,"wake repeated global inventory or skipped exact-slot checks")
print("native adapter: real Loaded inventory, private phases, one bind search, two business slots, readonly mailbox, off teardown")
