local root=assert(arg[1])
local ns={Release='2.0.2',Startup={ready=true,identity={product='retail',build='12.1.0.12345'}}}
local focus, frame, draws=nil,nil,{}
local combat=false
local actor={character='Paladin',realm='Realm',guid='Player-1-123'}
local callbacks={}
EventRegistry={
 RegisterCallback=function(_,event,callback,owner) assert(not callbacks[owner]);callbacks[owner]={event=event,callback=callback} end,
 UnregisterCallback=function(_,event,owner) assert(callbacks[owner] and callbacks[owner].event==event);callbacks[owner]=nil end,
}
local function fire(event)
 local copy={};for owner,v in pairs(callbacks) do if v.event==event then copy[#copy+1]={owner=owner,callback=v.callback} end end
 for _,v in ipairs(copy) do v.callback(v.owner) end
end
UIParent={GetEffectiveScale=function() return 1 end}
CreateFrame=function()
 assert(not frame,'only one optical card');frame={events={}}
 function frame:Hide() self.visible=false end
 function frame:Show() self.visible=true end
 function frame:EnableMouse() end
 function frame:SetFrameStrata() end
 function frame:SetPoint() end
 function frame:SetSize() end
 function frame:UnregisterAllEvents() self.events={} end
 function frame:RegisterEvent(event) self.events[event]=true end
 function frame:SetScript(_,fn) self.callback=fn end
 function frame:CreateTexture()
  return setmetatable({},{__index=function() return function() end end})
 end
 return frame
end
ns.Platform={ObserveActor=function() return actor end,ObserveInputState=function()
 if combat then return false,'input_combat_lockdown' end
 if focus then return false,'input_keyboard_focus' end;return true
end}
-- Only graphics encoding is a fixture. Controls, session, persistence, probe,
-- queue, reentry and the optical lifetime all use the production modules.
ns.MatrixSymbol={Encode=function(bytes)
 draws[#draws+1]=bytes;local matrix={};for x=1,21 do matrix[x]={};for y=1,21 do matrix[x][y]=0 end end;return matrix
end}
for _,name in ipairs({'Core/Persistence.lua','Bridge/CaptureWriter.lua','Bridge/Session.lua',
 'Bridge/ReceiptView.lua','Bridge/ReportStore.lua','Bridge/ProbeRunner.lua','Bridge/Reentry.lua','Core/Controls.lua'}) do
 assert(loadfile(root..'/'..name))('Lychee Dev',ns)
end
assert(ns.Persistence.Load())
assert(frame==nil and next(callbacks)==nil,'disabled bridge allocated optical resources')
assert(ns.Controls.Handle('bridge on'))
local nonce,reloadNonce=string.rep('a',32),string.rep('b',32)
assert(ns.Session.Bind(nonce))
local code='recoveryExecutions=(recoveryExecutions or 0)+1;return 55'
ns.ProbeDefinitions={schema='lycheedev.queue.v1',entries={['Recovery-A']={
 release=ns.Release,sessionNonce=nonce,reloadNonce=reloadNonce,character=actor.character,realm=actor.realm,guid=actor.guid,
 product='retail',build='12.1.0.12345',code=code,codeBytes=#code,codeSHA256=string.rep('c',64),codeAdler32=ns.CaptureWriter.DigestBytes(code),
}}}
assert(loadfile(root..'/Bridge/ProbeQueue.lua'))('Lychee Dev',ns)
local function gain() focus={};fire('ChatFrame.OnEditBoxFocusGained');assert(not frame.visible,'stale readiness remains visible') end
local function lose() focus=nil;fire('ChatFrame.OnEditBoxFocusLost') end
local function cycle(label)
 local before=draws[#draws];gain();lose()
 assert(frame.visible,label..': receipt never recovers after idle chat cycle')
 assert(draws[#draws]~=before,label..': stale readiness was reused')
end
local loaded=assert(ns.Controls.Handle('bridge load Recovery-A'))
assert(recoveryExecutions==nil and frame.visible)
cycle('loaded')
assert(recoveryExecutions==nil and draws[#draws]:find('"kind":"loaded"',1,true))
local report=assert(ns.Controls.Handle('bridge run Recovery-A'))
assert(recoveryExecutions==1)
cycle('reported')
assert(recoveryExecutions==1 and draws[#draws]:find('"receipt":'..report,1,true),'restoration executed probe or changed report')
-- The CLI opens chat itself; a rejected command must not permanently consume
-- the last readiness. No combat or player activity is involved.
gain();assert(ns.Controls.Handle('bridge reload Missing-Request')==nil);lose()
assert(frame.visible and recoveryExecutions==1,'rejected reload stranded the optical channel')
-- Combat and a loading screen suspend the optical channel, not the operation.
for _,events in ipairs({{'PLAYER_REGEN_DISABLED','PLAYER_REGEN_ENABLED'}, {'LOADING_SCREEN_ENABLED','LOADING_SCREEN_DISABLED'}}) do
 local before=draws[#draws]
 assert(frame.events[events[1]])
 frame.callback(frame,events[1]);assert(not frame.visible)
 assert(frame.events[events[2]], 'missing bounded recovery listener')
 frame.callback(frame,events[2])
 assert(frame.visible and draws[#draws]~=before,'transient state permanently lost report')
 assert(recoveryExecutions==1 and draws[#draws]:find('"receipt":'..report,1,true),'recovery replayed probe or changed report')
end
-- Loading can overlap combat; ending one condition must not publish permission.
combat=true;frame.callback(frame,'PLAYER_REGEN_DISABLED')
frame.callback(frame,'LOADING_SCREEN_ENABLED')
frame.callback(frame,'PLAYER_REGEN_ENABLED');assert(not frame.visible)
frame.callback(frame,'LOADING_SCREEN_DISABLED');assert(not frame.visible)
combat=false;frame.callback(frame,'PLAYER_REGEN_ENABLED');assert(frame.visible)
-- Zoning can include leaving/entering world, without a Lua runtime change.
frame.callback(frame,'LOADING_SCREEN_ENABLED');frame.callback(frame,'PLAYER_LEAVING_WORLD')
frame.callback(frame,'LOADING_SCREEN_DISABLED');assert(not frame.visible)
frame.callback(frame,'PLAYER_ENTERING_WORLD');assert(frame.visible and recoveryExecutions==1)
-- Combat during a chat suspension cannot discard the pending producer.
gain();combat=true;frame.callback(frame,'PLAYER_REGEN_DISABLED');lose();assert(not frame.visible)
combat=false;frame.callback(frame,'PLAYER_REGEN_ENABLED');assert(frame.visible)
-- Simulate a real reentry, retaining SV and queue but not runtime session.
InCombatLockdown=function() return false end
C_UI={Reload=function() end}
assert(ns.Controls.Handle('bridge reload Recovery-A')=='reload_requested')
assert(not frame.visible and next(callbacks)==nil)
assert(loadfile(root..'/Bridge/Session.lua'))('Lychee Dev',ns)
assert(loadfile(root..'/Bridge/Reentry.lua'))('Lychee Dev',ns)
local loader={RegisterEvent=function() end,UnregisterAllEvents=function() end,SetScript=function(self,_,fn) self.callback=fn end}
assert(ns.Reentry.Start(loader));loader.callback(loader,'PLAYER_ENTERING_WORLD',false,true)
assert(ns.Session.Current()==nil and not frame.visible,'reload ready before loading screen ended')
loader.callback(loader,'LOADING_SCREEN_DISABLED')
assert(ns.Session.Current().runtimeEpoch==2 and frame.visible)
cycle('reentry')
local ready=draws[#draws]
assert(ready:find('"requestId":"Recovery-A"',1,true) and ready:find(reloadNonce,1,true),'reentry lost exact correlation')
assert(recoveryExecutions==1)
gain();local stale={};for owner,v in pairs(callbacks) do stale[#stale+1]={owner=owner,callback=v.callback} end
assert(ns.Controls.Handle('bridge off'));lose()
for _,v in ipairs(stale) do v.callback(v.owner) end
assert(not frame.visible and next(callbacks)==nil,'opt-out resurrected readiness')
assert(ns.Controls.Handle('bridge on'));assert(ns.Session.Bind(nonce));assert(ns.Controls.Handle('bridge ready'))
gain();actor.guid='Player-other';lose()
assert(not frame.visible and next(callbacks)==nil,'actor change resurrected readiness')
actor.guid='Player-1-123';assert(ns.Session.Bind(nonce))
-- Equal pixels may belong to a new producer; memoization must not retain the
-- old owner's callback. A display replacement also cancels a suspended wait.
local oldRefresh=function() return 'obsolete' end
local newRefresh=function() return 'replacement' end
assert(ns.ReceiptView.Show('identical',nil,oldRefresh))
assert(ns.ReceiptView.Show('identical',nil,newRefresh))
gain();lose()
assert(draws[#draws]=='replacement','identical display retained obsolete producer')
gain();assert(ns.ReceiptView.Show('new owner',nil,function() return 'new owner refreshed' end));lose()
assert(draws[#draws]=='new owner','suspended callback overwrote replacement')
gain();assert(frame.events.PLAYER_LEAVING_WORLD);frame.callback(frame,'PLAYER_LEAVING_WORLD');lose()
assert(not frame.visible and next(callbacks)==nil,'world exit restored stale receipt')
for _,suspended in ipairs({false,true}) do
 assert(ns.ReceiptView.Show('before loading',nil,function() return 'stale loading receipt' end))
 if suspended then gain() end
 assert(frame.events.LOADING_SCREEN_ENABLED)
 frame.callback(frame,'LOADING_SCREEN_ENABLED');lose()
 assert(not frame.visible and next(callbacks)==nil,'loading start restored stale receipt')
end
-- Async completion can replace a suspended running receipt while chat still
-- has focus. Only the completed immutable report may survive the focus loss.
C_Timer={NewTimer=function() return {Cancel=function() end} end}
local asyncCode='local api=...;assert(api:Async(5));finishRecovery=function() return api:Finish(99) end'
assert(ns.ProbeRunner.Load('Recovery-Async',asyncCode,reloadNonce))
assert(ns.Controls.Handle('bridge run Recovery-Async'))
gain();local asyncReport=assert(finishRecovery());lose()
assert(frame.visible and draws[#draws]:find('"receipt":'..asyncReport,1,true),'async completion restored obsolete running receipt')
cycle('async reported')
assert(draws[#draws]:find('"receipt":'..asyncReport,1,true))
-- A changed actor and explicit dismissal permanently cancel environment waits.
frame.callback(frame,'PLAYER_REGEN_DISABLED');actor.guid='Player-other'
frame.callback(frame,'PLAYER_REGEN_ENABLED');assert(not frame.visible and next(frame.events)==nil)
actor.guid='Player-1-123';assert(ns.Session.Bind(nonce));assert(ns.Controls.Handle('bridge ready'))
frame.callback(frame,'LOADING_SCREEN_ENABLED');local staleEnvironment=frame.callback
ns.ReceiptView.Hide();staleEnvironment(frame,'LOADING_SCREEN_DISABLED')
assert(not frame.visible and next(frame.events)==nil,'dismissal resurrected receipt')
io.write('receipt recovery: passed')
