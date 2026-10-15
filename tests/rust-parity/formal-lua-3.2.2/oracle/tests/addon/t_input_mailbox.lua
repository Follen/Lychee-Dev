local Env=...
local ns=Env.LoadWorkbench()
local now,world,ready,reason=100,true,true,nil
GetTime=function()return now end
IsPlayerInWorld=function()return world end
ns.Platform={ObserveInputState=function()return ready,reason end}
ns.ReceiverBindings={Current=function()return {wake="ALT-CTRL-F12",reload="ALT-CTRL-F11"}end}
local frame
local create=CreateFrame
CreateFrame=function(...)frame=create(...);return frame end
local count=Env.framesCreated
for _,name in ipairs({"CaptureWriter","MemoryProtocol","InputState"})do Env.LoadAddon("Bridge/"..name..".lua",ns)end
assert(Env.framesCreated==count and ns.InputSignal==nil,"disabled input created optical or recurring work")
local identity={runtime=string.rep("1",32),owner="",fence=0,nextSlot=1,guid="g",build="b"}
local function provider()if not identity then return nil end;local copy={};for k,v in pairs(identity)do copy[k]=v end;return copy end
local mailbox={}
ns.InputState.Start(provider,mailbox)
assert(Env.framesCreated==count+1 and #frame.children==0,"mailbox sampler created visual elements")
assert(not frame:GetScript("OnKeyDown") and frame.mouseEnabled==false)
local function published(blocked,why)
    local record=assert(mailbox.input)
    assert(record==ns.InputState.Snapshot())
    assert(record:find('"schema":"Lycheedev mailbox protocol v1"',1,true))
    assert(record:find('"inputBlocked":'..tostring(blocked),1,true))
    assert(record:find('"escapeLimit":0',1,true))
    if why then assert(record:find('"reason":"'..why..'"',1,true))end
end
local function absent()assert(mailbox.input==nil and ns.InputState.Snapshot()==nil,"stale input authority retained")end
published(false)
ready,reason=false,"input_keyboard_focus";ns.InputState.Refresh();published(true,reason)
ready,reason=false,"input_combat_lockdown";ns.InputState.Refresh();published(true,reason)
ready,reason=Env.MakeSecret(),Env.MakeSecret();ns.InputState.Refresh();published(true,"input_observation_unavailable")
world=Env.MakeSecret();ns.InputState.Refresh();absent()
world=false;ns.InputState.Refresh();absent()
world=true;ready,reason=true,nil
local event=frame:GetScript("OnEvent")
event(frame,"LOADING_SCREEN_ENABLED");absent()
ns.InputState.Refresh();absent()
event(frame,"PLAYER_ENTERING_WORLD");absent()
event(frame,"LOADING_SCREEN_DISABLED");published(false)
event(frame,"PLAYER_LEAVING_WORLD");absent()
event(frame,"PLAYER_ENTERING_WORLD");published(false)
UIParent:Hide();published(false)
ready,reason=false,"input_keyboard_focus"
ns.InputState.Refresh();published(true,reason)
UIParent:Show();published(true,reason)
identity=nil;ns.InputState.Refresh();absent()
identity={runtime=string.rep("2",32),owner="",fence=0,nextSlot=1,guid="g2",build="b"}
local observe=ns.Platform.ObserveInputState
ns.Platform.ObserveInputState=function()error("API failure")end
ns.InputState.Refresh();absent()
ns.Platform.ObserveInputState=observe
now=99;ns.InputState.Refresh();absent()
now=101;ns.InputState.Refresh();published(true,reason)
local lateUpdate=frame:GetScript("OnUpdate")
ns.InputState.Stop();absent()
assert(not frame:GetScript("OnUpdate") and not frame:GetScript("OnEvent") and not frame.registeredEvent)
lateUpdate(frame,1);event(frame,"PLAYER_ENTERING_WORLD");ns.InputState.Refresh();absent()
ns.InputState.Start(provider,mailbox);assert(Env.framesCreated==count+1);published(true,reason)
ns.InputState.Stop();absent()
print("mailbox input: state, loading, secrets and teardown without optical cells")
