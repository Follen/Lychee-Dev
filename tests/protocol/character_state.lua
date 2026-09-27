local root=assert(arg[1])
local ns={}
issecretvalue=function() return false end
CreateFrame=function() error("storage allocated UI") end
local oldReport={receipt="old evidence",body="keep"}
local oldTicket={requestId="old work"}
LycheeToolkitDB={schema=1,options={bridgeEnabled=true},reports={old=oldReport},reentry=oldTicket,runtimeEpoch=92}
LycheeToolkitBridgeDB=nil
assert(loadfile(root.."/Core/Persistence.lua"))("Lychee Dev",ns)
assert(ns.Persistence.Load())
local account=ns.Persistence.Current()
local first=ns.Persistence.Bridge()
assert(account==LycheeToolkitDB and first~=account and first.schema==1)
assert(next(first.reports)==nil and first.reentry==nil and first.runtimeEpoch==nil,"old execution state imported")
first.reports.new={receipt="new receipt",body="new data"}
assert(account.reports.old==oldReport and account.reports.new==nil and account.reentry==oldTicket and account.runtimeEpoch==92)
-- Character switch creates an independent root; no mutable alias to account
-- or to the other character's report/ticket tables survives.
LycheeToolkitBridgeDB=nil
assert(ns.Persistence.Load())
local second=ns.Persistence.Bridge()
assert(first~=second and first.reports~=second.reports and next(second.reports)==nil)
assert(first.reports.new.body=="new data")
local future={schema=2,keep=true}
LycheeToolkitBridgeDB=future
assert(ns.Persistence.Load()==nil and LycheeToolkitBridgeDB==future and account.reports.old==oldReport)
assert(ns.Persistence.Bridge()==nil)
print("character storage isolation passed")
