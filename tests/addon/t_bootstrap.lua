local Env=...
local function setup(scenario)
 local ns=Env.LoadWorkbench()
 Env.LoadAddon('Bridge/CaptureWriter.lua',ns)
 Env.LoadAddon('Bridge/MemoryProtocol.lua',ns)
 local callbacks,created={}
 local original=CreateFrame
 CreateFrame=function(...)local f=original(...);created=f;return f end
 IsLoggedIn=function()return true end
 ns.ReceiverBindings={Current=function()return {wake='ALT-CTRL-F12',reload='ALT-CTRL-F11'}end,
  Register=function(value)callbacks=value;return true end}
 if scenario~='missing' then ns.StartRuntime=function()
  if scenario=='exception' then error('workbench module failed') end
  if scenario=='failure' then return nil,'state_invalid_root' end
  ns.Startup.ready=true;return true
 end end
 Env.LoadAddon('Core/Runtime.lua',ns)
 CreateFrame=original
 assert(ns.BootstrapMailbox and ns.BootstrapMailbox.bootstrap:sub(1,8)=='LYCMEM06')
 assert(not ns.Startup.ready and created:GetScript('OnUpdate')==nil)
 created:GetScript('OnEvent')(created,'ADDON_LOADED','OtherAddon')
 assert(not callbacks.reload)
 created:GetScript('OnEvent')(created,'ADDON_LOADED','Lychee Dev')
 assert(callbacks.wake and callbacks.reload)
 if scenario=='ready' then assert(ns.Startup.ready and created:GetScript('OnEvent')==nil)
 else
  assert(not ns.Startup.ready and ns.BootstrapMailbox.bootstrap:find('"phase":"failed"',1,true))
  assert(not callbacks.wake())
  created:GetScript('OnEvent')(created,'PLAYER_LOGIN')
  assert(created:GetScript('OnEvent')==nil)
 end
 Env.reloadCalled=false
 assert(callbacks.reload() and Env.reloadCalled,'business failure disabled manual reload')
 Env.inCombat=true;Env.reloadCalled=false
 assert(not callbacks.reload() and not Env.reloadCalled)
 Env.inCombat=false
end
for _,scenario in ipairs({'ready','missing','failure','exception'})do setup(scenario)end
print('bootstrap: independent reload, diagnostic publication and bounded lifecycle')
