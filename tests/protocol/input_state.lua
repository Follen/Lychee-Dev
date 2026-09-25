local root = assert(arg[1])
local secret = {}
issecretvalue = function(value) return rawequal(value,secret) end
CreateFrame = function() error("input observation must not create frames") end
local products = {"Live","Pandaria","Titan","Evergreen"}
    local builds = {Live={"12.1.0",120100},Pandaria={"5.5.4",50504},Titan={"3.80.2",38002},Evergreen={"1.60.1",16001}}
for _,product in ipairs(products) do
    local ns = {}
    do local v,i = unpack(builds[product]); GetBuildInfo = function() return v, "12345", "date", i end end
    assert(loadfile(root.."/Core/ClientGate.lua"))("Lychee Dev",ns)
    assert(loadfile(root.."/Core/Platform.lua"))("Lychee Dev",ns)
    local loggedIn,combat,focus = true,false,nil
    local counts = {login=0,combat=0,focus=0}
    IsLoggedIn = function() counts.login=counts.login+1;return loggedIn end
    InCombatLockdown = function() counts.combat=counts.combat+1;return combat end
    GetCurrentKeyBoardFocus = function() counts.focus=counts.focus+1;return focus end
    assert(counts.login==0 and counts.combat==0 and counts.focus==0)
    assert(ns.Platform.ObserveInputState()==true)
    loggedIn=false
    local ready,reason=ns.Platform.ObserveInputState()
    assert(ready==false and reason=="input_not_logged_in" and counts.combat==1 and counts.focus==1)
    loggedIn=true;combat=true
    ready,reason=ns.Platform.ObserveInputState()
    assert(ready==false and reason=="input_combat_lockdown" and counts.focus==1)
    combat=false;focus={}
    ready,reason=ns.Platform.ObserveInputState()
    assert(ready==false and reason=="input_keyboard_focus")
    focus=nil
    assert(ns.Platform.ObserveInputState()==true)
    for _,value in ipairs({secret,"unexpected",42,{}}) do
        loggedIn=value
        ready,reason=ns.Platform.ObserveInputState()
        assert(ready==nil and reason=="input_login_unavailable")
        loggedIn=true;combat=value
        ready,reason=ns.Platform.ObserveInputState()
        assert(ready==nil and reason=="input_combat_unavailable")
        combat=false
    end
    focus=secret
    ready,reason=ns.Platform.ObserveInputState()
    assert(ready==nil and reason=="input_focus_unavailable")
    GetCurrentKeyBoardFocus=function() error("fixture") end
    ready,reason=ns.Platform.ObserveInputState()
    assert(ready==nil and reason=="input_focus_unavailable")
    GetCurrentKeyBoardFocus=nil
    ready,reason=ns.Platform.ObserveInputState()
    assert(ready==nil and reason=="input_observation_unavailable")
end
io.write("input state: four profiles passed")
