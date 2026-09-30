local Env,client,root=...
local ns={}
for _,file in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol"}) do
    assert(loadfile(root.."/Bridge/"..file..".lua"))("Lychee Dev",ns)
end
local zero,runtime=string.rep("0",32),string.rep("1",32)
local function u32(n)return string.char(n%256,math.floor(n/256)%256,math.floor(n/65536)%256,math.floor(n/16777216)%256)end
local function raw(hex)return (hex:gsub("..",function(pair)return string.char(tonumber(pair,16))end))end
-- Independent original framing oracle; checksums run on final bytes.
local function original(nonce,run,ticket,kind,state,sequence,payload)
    local header="LYCMEM05"..raw(nonce)..raw(run)..raw(ticket)..string.char(kind,state,0,0)
        ..u32(sequence)..u32(#payload)..u32(ns.MemoryProtocol.Checksum(payload))..u32(80)
    return header..u32(ns.MemoryProtocol.Checksum(header))..payload.."LYCEND05"..raw(nonce)..raw(run)
end
for _,run in ipairs({runtime,string.rep("f",32),runtime}) do
    for kind=1,5 do
        for _,seq in ipairs({0,1,4294967295}) do
            for _,payload in ipairs({"","\0\255\n","荔枝\\\""}) do
                for _,nonce in ipairs({zero,run,string.rep("3",32)}) do
                    local ticket=kind==5 and zero or string.rep("4",32)
                    assert(ns.MemoryProtocol.Encode(nonce,run,ticket,kind,255,seq,payload)==original(nonce,run,ticket,kind,255,seq,payload))
                end
            end
        end
    end
end
local maximum=string.rep("x",524288)
assert(ns.MemoryProtocol.Encode(runtime,runtime,zero,3,3,4294967295,maximum)==original(runtime,runtime,zero,3,3,4294967295,maximum))
local function invalid(nonce,run,ticket,kind,state,seq,payload)
    local ok,value,reason=pcall(ns.MemoryProtocol.Encode,nonce,run,ticket,kind,state,seq,payload)
    assert(ok and not value and reason=="memory_record_invalid")
end
invalid(zero,runtime,zero,5,1,1,string.rep("x",524289))
invalid(zero,runtime,zero,nil,1,1,"")
invalid(zero,runtime,zero,1,1.5,1,"")
invalid(zero,runtime,zero,1,1,4294967296,"")
for _,position in ipairs({1,2,3,4,5,6,7}) do
    local values={runtime,runtime,zero,5,1,1,"input"}
    Env.secrets[values[position]]=true
    invalid(unpack(values));Env.secrets[values[position]]=nil
end

local a={character="荔枝\"\n\0",realm="服\\",guid="Player-1-1"}
local actorCalls,jsonCalls,wireCalls=0,0,0
local wire=ns.MemoryProtocol.Encode
ns.MemoryProtocol.Encode=function(...)wireCalls=wireCalls+1;return wire(...)end
local adapter={runtime=runtime,build="120100.69933",product=client,release="3.0.2",inventory=200,inputState="lycheedev.input.hybrid.v1",
    actor=function()actorCalls=actorCalls+1;return a end,
    encode=function(...)jsonCalls=jsonCalls+1;return ns.CaptureWriter.Encode(...)end,
    compile=loadstring,execute=function()error("not a business test")end}
local engine=ns.SlotProtocol.Create(adapter)
local seq=0
local function expected()
    local id=assert(engine.InputIdentity())
    id.schema="lycheedev.slot.identity.v1";id.slots=200
    id.product=adapter.product;id.release=adapter.release;id.inventory=adapter.inventory;id.inputState=adapter.inputState
    return original(zero,adapter.runtime,zero,1,1,seq,assert(ns.CaptureWriter.Encode(id,16384)))
end
local initial=assert(engine.Describe());assert(initial==expected())
local observed=actorCalls
for _=1,100 do assert(engine.Describe()==initial) end
assert(actorCalls==observed+100 and jsonCalls==1 and wireCalls==1,"reuse bypassed actor observation or repeated encoding")
local bind={schema="lycheedev.slot.v2",index=1,runtime=runtime,owner=string.rep("2",32),fence=1,
    nonce=string.rep("3",32),ticket=string.rep("4",32),action="bind",guid=a.guid,build=adapter.build}
assert(engine.Receive(1,bind));seq=1
local inside=engine.Snapshot().descriptor
local count=jsonCalls
local after=assert(engine.Describe())
assert(inside~=after and after==expected() and jsonCalls==count,"sequence change did not reuse payload with fresh framing")
assert(inside:sub(85,#inside-40)==after:sub(85,#after-40))
for _,envelope in ipairs({false,{runtime=string.rep("f",32)}}) do
    engine.Receive(engine.NextSlot(),envelope or nil)
    assert(engine.Describe()==expected() and engine.Describe()~=after,"consumed invalid/empty slot retained old cursor")
end
local function control(action,owner,fence)
    return {schema="lycheedev.slot.v2",index=engine.NextSlot(),runtime=runtime,owner=owner,fence=fence,
        nonce=string.format("%032x",engine.NextSlot()+100),ticket=zero,action=action,guid=a.guid,build=adapter.build}
end
local rebound=control("bind",bind.owner,2)
assert(engine.Receive(rebound.index,rebound));seq=seq+1;assert(engine.Describe()==expected())
local rejected=control("bind",string.rep("5",32),2)
local receipt,value=engine.Receive(rejected.index,rejected)
assert(receipt and value.reason=="slot_owner_busy");seq=seq+1;assert(engine.Describe()==expected())
local foreign=control("bind",bind.owner,2);foreign.runtime=string.rep("f",32)
local receipt,reason,skip=engine.Receive(foreign.index,foreign)
assert(not receipt and reason=="slot_skipped_foreign" and skip);assert(engine.Describe()==expected())
local unbind=control("unbind",bind.owner,2)
assert(engine.Receive(unbind.index,unbind));seq=seq+1;assert(engine.Describe()==expected())
-- Every output field and disappearance participates in the identity key.
for _,key in ipairs({"build","product","release","inventory","inputState","runtime"}) do
    local previous=adapter[key]
    adapter[key]=key=="inventory" and 199 or key=="runtime" and string.rep("a",32) or "changed\n\""
    assert(engine.Describe()==expected())
    adapter[key]=nil
    local record,reason=engine.Describe()
    if key=="runtime" then assert(not record) else assert(record==expected()) end
    adapter[key]=previous;assert(engine.Describe()==expected())
end
for _,key in ipairs({"character","realm","guid"}) do
    local previous=a[key];a[key]=previous.."new";assert(engine.Describe()==expected())
    Env.secrets[a[key]]=true
    local record,reason=engine.Describe();assert(not record and reason=="slot_actor_unavailable")
    Env.secrets[a[key]]=nil;a[key]=previous;assert(engine.Describe()==expected())
end
for _,bad in ipairs({"\255",string.rep("x",16385)}) do
    a.character=bad;local record,reason=engine.Describe();assert(not record and reason=="slot_identity_encoding")
end
a.character="Tester";assert(engine.Describe()==expected())
Env.secrets[adapter.release]=true
local record,reason=engine.Describe();assert(not record and reason=="slot_identity_encoding")
Env.secrets[adapter.release]=nil;assert(engine.Describe()==expected())
adapter.inventory={n=200};assert(engine.Describe()==expected())
adapter.inventory.n=199;assert(engine.Describe()==expected(),"mutable field incorrectly reused")
adapter.inventory=200
-- New engine is a reload/relogin/upgrade boundary: no descriptor cache leaks.
local replacement=ns.SlotProtocol.Create(adapter)
assert(replacement.Describe()~=engine.Describe())
print("publication: original framing parity, fresh actor, bounded current identity reuse")
