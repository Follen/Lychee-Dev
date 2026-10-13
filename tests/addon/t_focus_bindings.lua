local Env=...
local ns=Env.LoadWorkbench()
local focused,combat,events,removed=nil,false,0,0
local profile={wake="ALT-CTRL-F12",reload="ALT-CTRL-F11"}
local callbacks={}
GetCurrentKeyBoardFocus=function()return focused end
InCombatLockdown=function()return combat end
ns.ReceiverBindings={Current=function()return profile end}
ns.Compat.ReceiverChord=function(key)return key:match('^F%d+$') and 'ALT-CTRL-'..key end
EventRegistry.RegisterCallback=function(_,event,fn,owner)events=events+1;callbacks[event]=function()fn(owner)end end
EventRegistry.UnregisterCallback=function(_,event)removed=removed+1;callbacks[event]=nil end
local function editor()
 local f={propagate=false,originalCalls=0}
 f.original=function()f.originalCalls=f.originalCalls+1 end;f.script=f.original
 function f:IsForbidden()return false end
 function f:IsProtected()return self.protected or false end
 function f:IsPassword()return self.password or false end
 function f:IsSecureText()return self.secure or false end
 function f:GetScript()return self.script end
 function f:SetScript(_,value)self.script=value end
 function f:GetPropagateKeyboardInput()return self.propagate end
 function f:SetPropagateKeyboardInput(value)self.propagate=value end
 return f
end
local f=editor();focused=f
local frames=Env.framesCreated
Env.LoadAddon('Bridge/FocusBindings.lua',ns)
assert(events==0 and f.script==f.original and Env.framesCreated==frames)
ns.FocusBindings.Start();assert(events==2 and ns.FocusBindings.Ready())
ns.FocusBindings.Start();assert(events==2)
f.script(f,'A');assert(not f.propagate and f.originalCalls==1)
f.script(f,'F12');assert(f.propagate and f.originalCalls==2)
f.script(f,'B');assert(not f.propagate)
f.script(f,'F11');assert(f.propagate)
combat=true;f.script(f,'F12');assert(not f.propagate);combat=false
local saved=profile;profile=nil;f.script(f,'F12');assert(not f.propagate);profile=saved
local other=editor();focused=other
callbacks['ChatFrame.OnEditBoxFocusGained']()
assert(f.script==f.original and not f.propagate and ns.FocusBindings.Ready())
ns.FocusBindings.Stop()
assert(removed==2 and other.script==other.original and not other.propagate and not ns.FocusBindings.Ready())
for _,kind in ipairs({'password','protected','secure'})do
 local unsafe=editor();unsafe[kind]=true;focused=unsafe
 ns.FocusBindings.Start();assert(not ns.FocusBindings.Ready() and unsafe.script==unsafe.original)
 ns.FocusBindings.Stop()
end
focused=editor();ns.FocusBindings.Start()
local external=function()end;focused.script=external
ns.FocusBindings.Stop();assert(focused.script==external,'stop replaced another handler')
assert(Env.framesCreated==frames,'forwarding allocated extra frames')
print('focused binding forwarding: exact keys, original typing, restricted fields, lifecycle')
