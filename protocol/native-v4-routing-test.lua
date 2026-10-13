local root=arg[1]or"."
local ns={}
for _,path in ipairs({"CaptureWriter","NativeWire","NativeProtocol","NativeRuntime"})do assert(loadfile(root.."/addon/Bridge/"..path..".lua"))("Lychee Dev",ns)end
local function fixture(lane)
    local c={lane=lane,challenge=0,nonce=0,loads=0,executions=0,files={},runtime=string.format("%032x",lane+1),epoch=string.rep("e",32),owner=string.format("%032x",lane+100),ticket=string.rep("a",32)}
    local a={runtime=c.runtime,build="fixture",release="3.1.2",inventory=500,actor=function()return{guid="actor"}end,clock=function()return 1 end,
        challenge=function()c.challenge=c.challenge+1;return tostring(c.challenge)end,encode=ns.CaptureWriter.Encode,compile=function(code)return assert(loadstring(code))end,
        execute=function(fn,budget,done)c.executions=c.executions+1;c.done=done;return nil end,
        load=function(name)c.loads=c.loads+1;local slot=tonumber(name:match("(%d+)$"));return c.runtimeAdapter.Receive(slot,c.files[slot])end}
    c.runtimeAdapter=ns.NativeRuntime.Create(a);c.protocol=c.runtimeAdapter.Protocol
    function c.envelope(action,slot,fields)
        c.nonce=c.nonce+1
        local v={schema="lycheedev.slot.v4",runtime=c.runtime,owner=c.owner,sessionEpoch=c.epoch,fence=1,index=slot,nonce=string.format("%032x",c.nonce),
            ticket=c.ticket,requestSeq=c.requestSeq or 1,digest="digest",action=action,guid="actor",build="fixture",challenge=tostring(c.challenge)}
        for k,x in pairs(fields or{})do v[k]=x end;return v
    end
    assert(c.runtimeAdapter.Receive(lane*2+1,c.envelope("open",lane*2+1,{lane=lane}),true))
    function c.execute()
        local slot=c.protocol.NextSlot();c.files[slot]=c.envelope("execute",slot,{code="return 7",budget=30,expires=100})
        assert(c.runtimeAdapter.Wake());return slot
    end
    function c.report()assert(c.done(true,{answer=7},{resourcesReleased=true}))end
    function c.settle()
        local slot=c.protocol.NextSlot();local payload=assert(ns.CaptureWriter.Encode({ok=true,result={answer=7},resourcesReleased=true},524288))
        c.files[slot]=c.envelope("settle",slot,{resultBytes=#payload,resultChecksum=ns.NativeWire.Checksum(payload)})
        assert(c.runtimeAdapter.Settle())
    end
    function c.close()local slot=c.protocol.NextSlot();c.files[slot]=c.envelope("close",slot);assert(c.runtimeAdapter.Close())end
    return c
end
local A,B=fixture(0),fixture(1)
local aSlot,bSlot=A.execute(),B.execute()
for i=1,1000 do assert(not A.runtimeAdapter.Wake());assert(not A.runtimeAdapter.Settle())end
assert(A.loads==1 and A.protocol.NextSlot()==aSlot+1)
A.report();B.report()
-- A late submit after reported can consume one empty control slot, then is fused.
assert(not A.runtimeAdapter.Settle())
local fusedNext=A.protocol.NextSlot()
for i=1,1000 do assert(not A.runtimeAdapter.Settle());assert(not A.runtimeAdapter.Wake())end
assert(A.protocol.NextSlot()==fusedNext and A.loads==2)
-- Other instance consumes only its own route; no shared-spare exhaustion matters here.
B.settle();B.close();assert(B.loads==3 and B.protocol.NextSlot()==bSlot+3)
assert(A.runtimeAdapter.Receive(2,A.envelope("recover",2),true))
A.settle();A.close();assert(A.loads==4 and A.executions==1)
for i=1,1000 do assert(not A.runtimeAdapter.Close())end
assert(A.loads==4)
-- Wrong control action cannot dispatch business and blocks further raw controls.
local C=fixture(2);C.execute();C.report();local slot=C.protocol.NextSlot();C.files[slot]=C.envelope("execute",slot,{code="return 8",budget=30,expires=100})
assert(C.runtimeAdapter.Settle());assert(C.executions==1)
for i=1,1000 do assert(not C.runtimeAdapter.Settle())end
assert(C.loads==2)
assert(C.runtimeAdapter.Receive(6,C.envelope("recover",6),true));C.settle();C.close()
-- A subsequent task reported state cannot reuse its predecessor's submit nonce.
local D=fixture(3);D.execute();D.report();D.settle();local priorNonce=D.nonce;D.requestSeq=2;D.ticket=string.rep("b",32);D.execute();D.report()
local nextSlot=D.protocol.NextSlot();local bad=D.envelope("settle",nextSlot,{resultBytes=1,resultChecksum=1});bad.nonce=string.format("%032x",priorNonce);D.files[nextSlot]=bad
assert(not D.runtimeAdapter.Settle());local frozen=D.protocol.NextSlot();for i=1,1000 do assert(not D.runtimeAdapter.Settle())end;assert(D.protocol.NextSlot()==frozen)
assert(D.runtimeAdapter.Receive(8,D.envelope("recover",8),true));D.settle();D.close();assert(D.executions==2)
print("native-v4-routing: passed lanes=4 lateWake=1000 lateSubmit=1000 boundedControlFault=1 normalSlots=2")
