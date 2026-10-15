local Env=...
local ns=Env.LoadWorkbench()
local now,world,ready,reason=100,true,true,nil
GetTime=function()return now end
IsPlayerInWorld=function()return world end
ns.Platform={ObserveInputState=function()return ready,reason end}
ns.ReceiverBindings={Current=function()return {wake="ALT-CTRL-F12",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end}
ns.Compat.GetPhysicalPixelSize=function()return 1 end
local stops=0
ns.StartupBeacon={Stop=function()stops=stops+1 end}
local count=Env.framesCreated
for _,name in ipairs({"CaptureWriter","MemoryProtocol","InputSignal","InputState"}) do Env.LoadAddon("Bridge/"..name..".lua",ns) end
assert(Env.framesCreated==count,"disabled signal allocated frame")
ns.InputState.Refresh();assert(Env.framesCreated==count)
local identity={runtime=string.rep("1",32),owner="",fence=0,nextSlot=1,guid="g",build="b"}
local function provider()if not identity then return nil end;local copy={};for k,v in pairs(identity)do copy[k]=v end;return copy end
local mailbox={}
ns.InputState.Start(provider,mailbox)
local frame=ns.InputSignal.Frame()
assert(Env.framesCreated==count+1 and #frame.children==3,"signal not one frame and three textures")
assert(not frame:GetScript("OnKeyDown") and frame.mouseEnabled==false)
local blocks=frame.children
local function color(block,r,g,b)
    local c=block.colorTexture
    assert(block:IsShown() and c[1]==r and c[2]==g and c[3]==b and c[4]==1,"wrong code color")
end
local function hidden()for _,b in ipairs(blocks)do assert(not b:IsShown(),"stale color retained")end;assert(not ns.InputState.Snapshot() and mailbox.input==nil)end
color(blocks[1],0,1,0);color(blocks[2],1,1,1)
assert(frame.width==6 and frame.height==2 and blocks[1].width==2 and blocks[1].height==2)
local hb=blocks[3].colorTexture[1]
ns.InputState.Refresh()
assert(blocks[3].colorTexture[1]~=hb,"same-frame refresh did not commit heartbeat")
assert(ns.InputState.Snapshot():find('"sampleMillis":100000',1,true),"optical refresh changed memory clock")
assert(mailbox.input==ns.InputState.Snapshot())
ready,reason=false,"input_keyboard_focus";ns.InputState.Refresh();color(blocks[1],1,0,0);color(blocks[2],0,0,1)
ready,reason=false,"input_combat_lockdown";ns.InputState.Refresh();color(blocks[1],0,0,1);color(blocks[2],1,0,0)
ready,reason=Env.MakeSecret(),Env.MakeSecret();ns.InputState.Refresh();color(blocks[1],1,1,1);color(blocks[2],0,0,0)
assert(ns.InputState.Snapshot():find('"inputBlocked":true',1,true))
world=Env.MakeSecret();ns.InputState.Refresh();hidden()
world=false;ns.InputState.Refresh();hidden()
world=true;ready,reason=true,nil
local event=frame:GetScript("OnEvent")
event(frame,"LOADING_SCREEN_ENABLED");hidden()
ns.InputState.Refresh();hidden()
event(frame,"PLAYER_ENTERING_WORLD");hidden()
event(frame,"LOADING_SCREEN_DISABLED");color(blocks[1],0,1,0)
event(frame,"PLAYER_LEAVING_WORLD");hidden()
event(frame,"PLAYER_ENTERING_WORLD");color(blocks[1],0,1,0)
UIParent:Hide();hidden()
ready,reason=false,"input_keyboard_focus"
UIParent:Show();color(blocks[1],1,0,0)
identity=nil;ns.InputState.Refresh();hidden()
local observe=ns.Platform.ObserveInputState
identity={runtime=string.rep("2",32),owner="",fence=0,nextSlot=1,guid="g2",build="b"}
ns.Platform.ObserveInputState=function()error("API failure")end
ns.InputState.Refresh();hidden()
ns.Platform.ObserveInputState=observe
now=99;ns.InputState.Refresh();hidden()
now=101;ns.InputState.Refresh();color(blocks[1],1,0,0)
local lateUpdate=frame:GetScript("OnUpdate")
ns.InputState.Stop();hidden()
assert(stops==1 and not frame:GetScript("OnUpdate") and not frame:GetScript("OnEvent") and not frame.registeredEvent)
lateUpdate(frame,1);event(frame,"PLAYER_ENTERING_WORLD");ns.InputState.Refresh();hidden()
ns.InputState.Start(provider,mailbox);assert(Env.framesCreated==count+1 and mailbox.input==ns.InputState.Snapshot());ns.InputState.Stop();hidden()
-- The shared Go/Lua fixture fixes the legal pairs; one changed block cannot
-- turn one legal pair into another because both positions are unique.
local f=assert(io.open("../../protocol/input-color/golden.json","rb"));local fixture=f:read("*a");f:close()
local expected={ready='"green","white"',input_keyboard_focus='"red","blue"',input_combat_lockdown='"blue","red"',unknown='"white","black"'}
for state,pair in pairs(expected)do assert(fixture:find('"state":"'..state..'","colors":['..pair..']',1,true),"color fixture changed")end
print("hybrid signal: three cells, one lifecycle, no stale readiness")
