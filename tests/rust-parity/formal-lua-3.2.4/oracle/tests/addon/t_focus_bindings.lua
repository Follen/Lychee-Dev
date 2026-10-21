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
 local f={propagate=false,originalCalls=0,propagationWrites=0}
 f.original=function(self,key,...)
  f.originalCalls=f.originalCalls+1
  if f.onOriginal then f.onOriginal(self,key,...) end
 end;f.script=f.original
 function f:IsForbidden()return false end
 function f:IsProtected()return self.protected or false end
 function f:IsPassword()return self.password or false end
 function f:IsSecureText()return self.secure or false end
 function f:GetScript()return self.script end
 function f:SetScript(_,value)self.script=value end
 function f:GetPropagateKeyboardInput()return self.propagate end
 function f:SetPropagateKeyboardInput(value)
  assert(not combat,'[ADDON_ACTION_BLOCKED] ChatFrame1EditBox:SetPropagateKeyboardInput()')
  self.propagationWrites=self.propagationWrites+1
  self.propagate=value
 end
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
local writes,calls=f.propagationWrites,f.originalCalls
combat=true
f.script(f,'F12');f.script(f,'A')
assert(f.propagationWrites==writes and f.originalCalls==calls+2,'combat changed propagation or swallowed typing')
assert(not ns.FocusBindings.Ready() and not ns.FocusBindings.Refresh(),'combat advertised forwarding')
combat=false
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

-- Focus can change while combat prevents restoring the old propagation state.
f=editor();focused=f;ns.FocusBindings.Start();f.script(f,'F12')
other=editor();focused=other;combat=true
callbacks['ChatFrame.OnEditBoxFocusGained']()
assert(other.script==other.original and not ns.FocusBindings.Ready(),'attached a new editor in combat')
combat=false
assert(ns.FocusBindings.Refresh())
assert(f.script==f.original and not f.propagate,'focus change lost the original propagation state')
ns.FocusBindings.Stop()

-- Starting during combat waits for the existing INPUT refresh after combat.
focused=editor();combat=true;ns.FocusBindings.Start()
assert(focused.script==focused.original and not ns.FocusBindings.Ready())
combat=false;assert(ns.FocusBindings.Refresh());ns.FocusBindings.Stop()

-- The original handler still runs, and its state changes must be observed
-- before granting propagation for either registered shortcut.
focused=editor();focused.onOriginal=function()combat=true end
ns.FocusBindings.Start();focused.script(focused,'F12')
assert(focused.originalCalls==1 and not focused.propagate and focused.propagationWrites==1)
combat=false;ns.FocusBindings.Stop()
assert(Env.framesCreated==frames,'forwarding allocated extra frames')

-- A combat-time stop revokes INPUT immediately, then uses its existing frame
-- for one combat-end cleanup. There must be no sampler or new cleanup frame.
Env.LoadAddon('Bridge/CaptureWriter.lua',ns)
Env.LoadAddon('Bridge/MemoryProtocol.lua',ns)
Env.LoadAddon('Core/Platform.lua',ns)
Env.LoadAddon('Bridge/InputState.lua',ns)
IsPlayerInWorld=function()return true end
IsLoggedIn=function()return true end
GetTime=function()return 100 end
local sampler
local create=CreateFrame
CreateFrame=function(...)
 sampler=create(...);sampler.events={}
 function sampler:RegisterEvent(event)self.events[event]=true end
 function sampler:UnregisterAllEvents()self.events={} end
 return sampler
end
local provider=function()return {runtime=string.rep('1',32),owner='',fence=0,nextSlot=1,guid='g',build='b'}end
local mailbox={}
focused=editor();ns.InputState.Start(provider,mailbox)
assert(mailbox.input and sampler:GetScript('OnUpdate') and Env.framesCreated==frames+1)
focused.script(focused,'F12');assert(focused.propagate)
combat=true;ns.InputState.Stop()
assert(not mailbox.input and not ns.InputState.Snapshot() and not sampler:GetScript('OnUpdate'))
assert(not ns.FocusBindings.Ready() and not next(callbacks) and Env.framesCreated==frames+1)
local cleanup=assert(sampler:GetScript('OnEvent'),'lost deferred propagation cleanup')
assert(next(sampler.events)=='PLAYER_REGEN_ENABLED' and next(sampler.events,'PLAYER_REGEN_ENABLED')==nil)
writes,calls=focused.propagationWrites,focused.originalCalls
focused.script(focused,'A')
assert(focused.propagationWrites==writes and focused.originalCalls==calls+1)
combat=false;cleanup(sampler,'PLAYER_REGEN_ENABLED')
assert(focused.script==focused.original and not focused.propagate,'stop did not restore propagation after combat')
assert(not sampler:GetScript('OnEvent') and not next(sampler.events) and not mailbox.input)

-- Re-enabling before cleanup reuses the sampler and resumes normal teardown.
ns.InputState.Start(provider,mailbox);focused.script(focused,'F11')
combat=true;ns.InputState.Stop();ns.InputState.Start(provider,mailbox)
assert(sampler:GetScript('OnUpdate') and Env.framesCreated==frames+1)
combat=false;sampler:GetScript('OnEvent')(sampler,'PLAYER_REGEN_ENABLED')
assert(ns.FocusBindings.Ready() and mailbox.input)
ns.InputState.Stop()
assert(focused.script==focused.original and not focused.propagate)
assert(not sampler:GetScript('OnEvent') and not sampler:GetScript('OnUpdate') and not next(sampler.events))
focused=nil;ns.InputState.Start(provider,mailbox);combat=true;ns.InputState.Stop()
assert(not sampler:GetScript('OnEvent') and not next(sampler.events),'stop without an editor retained cleanup work')
combat=false
CreateFrame=create
print('focused binding forwarding: exact keys, typing, combat boundaries, deferred teardown')
