local root=arg[1] or "."
local ns={}
assert(loadfile(root.."/addon/Bridge/CaptureWriter.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/addon/Bridge/NativeWire.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/addon/Bridge/NativeProtocol.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/addon/Bridge/NativeRuntime.lua"))("Lychee Dev",ns)
local tokens={runtime=string.rep("1",32),owner=string.rep("2",32),epoch=string.rep("3",32),ticket=string.rep("4",32)}
local challenge,executions,encodes=0,0,0
local done
local p=ns.NativeProtocol.Create({runtime=tokens.runtime,build="fixture",release="3.1.2",inventory=500,
    actor=function()return{guid="actor"}end,clock=function()return 1 end,challenge=function()challenge=challenge+1;return tostring(challenge)end,
    encode=function(value,limit)encodes=encodes+1;return ns.CaptureWriter.Encode(value,limit)end,
    compile=function(code)return assert(loadstring(code))end,
    execute=function(fn,budget,finish)executions=executions+1;done=finish;return nil end})
local box={schema="lycheedev.mailbox.v2",runtime=tokens.runtime}
p.AttachMailbox(box)
local nextNonce=1
local function e(action,index,extra)
    local v={schema="lycheedev.slot.v4",runtime=tokens.runtime,owner=tokens.owner,sessionEpoch=tokens.epoch,fence=1,index=index,
        nonce=string.format("%032x",nextNonce),ticket=tokens.ticket,requestSeq=1,digest="digest",action=action,guid="actor",build="fixture",challenge=tostring(challenge)}
    nextNonce=nextNonce+1;for k,x in pairs(extra or {})do v[k]=x end;return v
end
assert(p.Receive(1,e("open",1,{lane=0}),true))
assert(p.NextSlot()==21)
local ex=e("execute",21,{code="return 7",budget=30,expires=100})
assert(p.Receive(21,ex,false));assert(executions==1)
ex.code="return 999" -- caller cannot mutate private execution identity
local publishedBodies=box.bodies
box.bodies={forged="data"};box.identity="fake";box.receipts={}
local _,reason=p.Receive(81,e("execute",81),false);assert(reason=="native_foreign_route")
assert(p.NextSlot()==22)
assert(not p.Receive(22,nil,false));assert(p.Fused())
for i=1,1000 do assert(not p.Receive(23,nil,false))end
assert(p.NextSlot()==23)
assert(done(true,{answer=7},{resourcesReleased=true})) -- async finish survives route fuse
assert(publishedBodies[tokens.ticket])
assert(p.Receive(2,e("recover",2),true));assert(not p.Fused())
local bad=e("execute",23,{code="return 999",budget=30,expires=100})
assert(p.Receive(23,bad,false));assert(executions==1)
local payload=assert(ns.CaptureWriter.Encode({ok=true,result={answer=7},resourcesReleased=true},524288))
assert(p.Receive(24,e("settle",24,{resultBytes=#payload,resultChecksum=ns.NativeWire.Checksum(payload)}),false,true))
assert(publishedBodies[tokens.ticket]==nil)
assert(p.Receive(25,e("execute",25,{code="return 7",budget=30,expires=100}),false));assert(executions==1)
assert(p.Receive(26,e("close",26),false,true))
print("native-v4: passed executions="..executions.." encodes="..encodes.." lateWakes=1000 foreign=0 normalSlots=2")
-- A syntactically invalid source is an observed failed report, as in the v3
-- business oracle, not an unavailable rejected request. It still needs SETTLE.
local badChallenge=0
local badExecutions=0
local compilePeer=ns.NativeProtocol.Create({runtime=tokens.runtime,build="fixture",release="3.1.2",inventory=500,
    actor=function()return{guid="actor"}end,clock=function()return 1 end,challenge=function()badChallenge=badChallenge+1;return tostring(badChallenge)end,
    encode=ns.CaptureWriter.Encode,compile=function(code)return loadstring(code)end,execute=function()badExecutions=badExecutions+1 end})
local compileBox={schema="lycheedev.mailbox.v2",runtime=tokens.runtime};compilePeer.AttachMailbox(compileBox)
local opened=e("open",1,{lane=0,challenge=tostring(badChallenge)});assert(compilePeer.Receive(1,opened,true))
local failed=e("execute",21,{code="local = broken",budget=30,expires=100,challenge=tostring(badChallenge)})
local receipt=assert(compilePeer.Receive(21,failed,false));assert(receipt:find('"state":"reported"',1,true));assert(badExecutions==0)
assert(compileBox.bodies[tokens.ticket]:find('"kind":"compile_error"',1,true))
assert(compilePeer.CanSettle() and not compilePeer.CanClose())
