local root = assert(arg[1])
local secret = {}
issecretvalue = function(v) return rawequal(v, secret) end
CreateFrame = function() error("epoch allocated runtime machinery") end
local ns = { Release="2.0.3", Startup={ready=true,identity={product="retail",build="12.1.0.12345"}},
    Platform={ObserveActor=function() return {character="Paladin",realm="Realm",guid="Player-1-123"} end,
        ObserveInputState=function() return true end} }
for _, file in ipairs({"Core/Persistence.lua","Bridge/Session.lua","Bridge/CaptureWriter.lua"}) do
    assert(loadfile(root.."/"..file))("Lychee Dev", ns)
end
assert(ns.Persistence.Load())
local state=LycheeToolkitDB
local nonce=string.rep("a",32)
assert(ns.Session.Bind(nonce)==nil and state.runtimeEpoch==nil)
state.options.bridgeEnabled=true
assert(ns.Session.Bind(nonce).runtimeEpoch==1)
local first=assert(ns.Session.ReadyReceipt())
assert(ns.Session.Bind(nonce).runtimeEpoch==1 and state.runtimeEpoch==1)
ns.Session.Release()
assert(ns.Session.Bind(nonce).runtimeEpoch==1)
-- Re-evaluate only the session module to model a new Lua runtime while keeping
-- SavedVariables. No old binding is restored merely by loading the module.
assert(loadfile(root.."/Bridge/Session.lua"))("Lychee Dev",ns)
assert(ns.Session.Current()==nil and state.runtimeEpoch==1)
assert(ns.Session.Bind(nonce).runtimeEpoch==2)
local second=assert(ns.Session.ReadyReceipt())
local replacement={schema=1,options={bridgeEnabled=true},reports={}}
LycheeToolkitDB=replacement
assert(ns.Session.Current()==nil)
assert(ns.Session.Bind(nonce).runtimeEpoch==3 and replacement.runtimeEpoch==3)
replacement.runtimeEpoch=secret
assert(ns.Session.Current()==nil and ns.Session.Bind(nonce)==nil)
assert(rawequal(replacement.runtimeEpoch,secret))
for _, value in ipairs({-1,1.5,"2",false,math.huge,9007199254740991,secret}) do
    LycheeToolkitDB={schema=1,options={bridgeEnabled=true},reports={},runtimeEpoch=value}
    assert(loadfile(root.."/Bridge/Session.lua"))("Lychee Dev",ns)
    assert(ns.Session.Bind(nonce)==nil and rawequal(LycheeToolkitDB.runtimeEpoch,value))
end
io.write(assert(ns.CaptureWriter.Encode({first=first,second=second})))
