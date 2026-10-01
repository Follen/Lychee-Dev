-- Experimental loader only. Real protocol/runtime modules, simulated file/LoD adapter.
local root=assert(arg[1])
assert(loadfile(root.."/tests/lib/wow_globals.lua"))()
local ns={}
for _,name in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol"}) do
    assert(loadfile(root.."/addon/Bridge/"..name..".lua"))("Lychee Dev",ns)
end
local schema="lycheedev.slotset.prototype.v1"
local A,B=string.rep("1",32),string.rep("2",32)
local compiled,executed=0,0
local function actor()return {guid="Player-1-1",character="Tester",realm="Realm"}end
local function engine(runtime)
    return ns.SlotProtocol.Create({runtime=runtime,build="70000",product="retail",release="test",
        actor=actor,encode=ns.CaptureWriter.Encode,
        compile=function()compiled=compiled+1;error("unexpected business compilation")end,
        execute=function()executed=executed+1;error("unexpected business execution")end})
end
local function member(runtime,owner)
    return {schema="lycheedev.slot.v3",index=1,runtime=runtime,owner=owner or runtime,fence=1,
        nonce=runtime,ticket=string.rep("0",32),action="bind",guid="Player-1-1",build="70000",
        code="error('must never run')"}
end
-- Parse failures are simulated with actual Lua 5.1 compilation. This is NOT an
-- untrusted-data parser: callers here supply fixture source, never external Lua.
local function parse(source)
    local fn,err=loadstring(source,"=collection-fixture")
    if not fn then return nil,"parse" end
    setfenv(fn,{})
    local ok,value=pcall(fn)
    if not ok then return nil,"parse" end
    return value
end
local function loader(e,runtime)
    local attempted={}
    return function(index,decode)
        if attempted[index] then return nil,"already_attempted" end
        attempted[index]=true
        local value,reason=decode()
        local chosen,count=nil,0
        if not reason then
            if type(value)~="table" or getmetatable(value) or value.schema~=schema
                or type(value.members)~="table" or #value.members>12 then reason="format"
            else
                for _,v in ipairs(value.members)do
                    if type(v)~="table" then reason="format";break end
                    if v.runtime==runtime then chosen=v;count=count+1 end
                end
                if not reason and count~=1 then reason=count==0 and "no_match" or "ambiguous" end
            end
        end
        -- Explicit new prototype responsibility: every attempted load gets a
        -- Receive, including decode failure. Existing SlotRuntime does not do it.
        if reason then
            local _,rejected=e.Receive(index,nil)
            assert(rejected=="slot_skipped_empty")
            return nil,reason
        end
        return e.Receive(index,chosen)
    end
end
local function literal(runtime)
    return string.format('{schema="lycheedev.slot.v3",index=1,runtime="%s",owner="%s",fence=1,nonce="%s",ticket="%s",action="bind",guid="Player-1-1",build="70000",code="error(123)"}',runtime,runtime,runtime,string.rep("0",32))
end
local sharedSource='return {schema="'..schema..'",members={'..literal(A)..','..literal(B)..'}}'
local set=assert(parse(sharedSource))
for _,runtime in ipairs({A,B})do
    local e=engine(runtime)
    local wake=loader(e,runtime)
    local record,value=wake(1,function()return parse(sharedSource) end)
    assert(record and value.state=="bound" and e.Snapshot().owner==runtime and e.NextSlot()==2)
    assert(not wake(1,function()error("reparsed consumed slot")end))
end
print("PASS two runtimes uniquely select from identical serialized collection bytes")
for _,case in ipairs({
    {"no_match",function()return {schema=schema,members={member(B)}}end},
    {"ambiguous",function()return {schema=schema,members={member(A),member(A,B)}}end},
    {"format",function()return {schema="other",members={member(A)}}end},
    {"parse",function()return parse("return {schema=")end},
})do
    local e=engine(A);local wake=loader(e,A)
    local ok,reason=wake(1,case[2])
    assert(not ok and reason==case[1] and e.NextSlot()==2 and not e.Snapshot().owner)
    local _,again=wake(1,function()return set end)
    assert(again=="already_attempted")
    print("PASS consumes before retry: "..reason)
end
-- Current production Receive accepts only the single-envelope schema, never a set.
local legacy=engine(A)
local _,reason=legacy.Receive(1,set)
assert(reason=="slot_envelope_invalid" and legacy.NextSlot()==2 and not legacy.Snapshot().owner)
print("PASS production single-envelope engine rejects outer collection schema")

-- Exercise actual SlotRuntime wake/expectedSlot cleanup with a throwing parser.
-- No WoW loader is present; this adapter reports attempted file evaluation only.
local state={options={bridgeEnabled=true}}
ns.Release="test"
ns.Persistence={Current=function()return state end,Bridge=function()return state end}
ns.Platform={ObserveBuild=function()return {build="70000",product="retail"}end,
    ObserveActor=actor,ObserveInputState=function()return true end}
ns.AutomationHistory={Begin=function()end,Observe=function()return true end}
ns.ProbeExecution={Run=function()executed=executed+1;error("business execution")end}
local active=false
ns.InputProtection={Acquire=function()active=true;return {}end,
    Release=function()active=false end,Cancel=function()active=false end,IsActive=function()return active end}
local loads=0
ns.Compat={MonotonicSeconds=function()return 100 end,
    GetAddOnMetadata=function(name,key)
        if key=="Version" then return "test" end
        if key=="X-Lychee-Transport" then return "memory-slot-v3" end
        return tostring(tonumber(name:match("(%d+)$")))
    end,
    LoadInputSlot=function()
        loads=loads+1
        assert(loadstring("return {schema="))()
    end}
assert(loadfile(root.."/addon/Bridge/SlotRuntime.lua"))("Lychee Dev",ns)
assert(ns.SlotRuntime.Start())
for i=1,2 do
    local ok,why=ns.SlotRuntime.Wake()
    assert(not ok and why=="slot_wake_failed" and not active)
    assert(not ns.SlotRuntime.Snapshot().consumed[1])
end
assert(loads==2)
local _,denied=ns.SlotRuntime.Receive(1,member(A))
assert(denied=="slot_not_requested")
assert(compiled==0 and executed==0)
print("PASS current runtime parse failure leaves engine slot unconsumed; receive capability released")
print("PASS zero business compilation/execution; real LoD state remains unverified")
