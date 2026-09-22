local root = assert(arg[1])
local ns = {Startup={ready=true}}
local state = {options={bridgeEnabled=true}}
local actor = {character="Paladin",realm="Realm",guid="Player-1-123"}
local ready,reason = false,"input_keyboard_focus"
ns.Persistence = {Current=function() return state end}
ns.Platform = {ObserveActor=function() return actor end,ObserveInputState=function() return ready,reason end}
CreateFrame = function() error("input wait must not create frames") end
local callbacks = {}
local registrations,removals,delivered = 0,0,0
EventRegistry = {
    RegisterCallback=function(self,event,callback,owner)
        assert(event=="ChatFrame.OnEditBoxFocusLost" and callbacks[owner]==nil)
        callbacks[owner]=callback;registrations=registrations+1
    end,
    UnregisterCallback=function(self,event,owner)
        assert(event=="ChatFrame.OnEditBoxFocusLost" and callbacks[owner])
        callbacks[owner]=nil;removals=removals+1
    end,
}
assert(loadfile(root.."/Bridge/Session.lua"))("Lychee Dev",ns)
assert(registrations==0)
assert(ns.Session.WhenInputReady(function() end)==nil)
local nonce=string.rep("a",32)
assert(ns.Session.Bind(nonce))
local result,failure
local function receive(value,why) delivered=delivered+1;result=value;failure=why end
local function fire()
    local owner,callback=next(callbacks)
    assert(callback);callback(owner)
    assert(next(callbacks)==nil)
    return callback
end
assert(ns.Session.WhenInputReady(receive))
assert(ns.Session.WhenInputReady(receive)==nil)
assert(delivered==0 and registrations==1)
ready,reason=true,nil
local stale=fire()
assert(delivered==1 and result==true and removals==1)
stale();assert(delivered==1)
assert(ns.Session.WhenInputReady(receive))
assert(delivered==2 and registrations==1)
ready,reason=false,"input_keyboard_focus"
assert(ns.Session.WhenInputReady(receive))
ready,reason=false,"input_combat_lockdown"
fire();assert(delivered==3 and result==false and failure==reason)
ready,reason=false,"input_keyboard_focus"
assert(ns.Session.WhenInputReady(receive))
actor={character="Other",realm="Realm",guid="Player-1-999"}
fire();assert(delivered==4 and result==false and failure=="session_actor_changed")
assert(ns.Session.Bind(nonce))
assert(ns.Session.WhenInputReady(receive))
ns.Session.Release();assert(next(callbacks)==nil and delivered==4)
assert(ns.Session.Bind(nonce))
assert(ns.Session.WhenInputReady(receive))
state.options.bridgeEnabled=nil
assert(ns.Session.Current()==nil and next(callbacks)==nil)
state.options.bridgeEnabled=true
assert(ns.Session.Bind(nonce))
assert(ns.Session.WhenInputReady(receive))
ns.Session.CancelInputWait();ns.Session.CancelInputWait()
assert(next(callbacks)==nil)
EventRegistry=nil
assert(ns.Session.WhenInputReady(receive)==nil)
assert(registrations==removals)
io.write("input wait: passed")
