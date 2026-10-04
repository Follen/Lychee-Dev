local F=assert(loadfile(arg[1].."/tests/protocol/duplex_fixture.lua"))()
local f=F.engine();collectgarbage("collect");local baseline=collectgarbage("count")
local wire
if arg[2] then
    local input=assert(io.open(arg[2],"rb"));wire=input:read("*a");input:close()
    assert(#wire==1048896 and F.P.VerifyFrame(wire:sub(1,320),wire:sub(321)),"Go megabyte row invalid")
end
local source="return true"..string.rep(" ",1048576-11)
wire=F.header(F.P.Kind.frame,source,{challenge=f.engine.Snapshot().readyChallenge})
F.setRow(f.arena.command,wire);source=nil;wire=nil;collectgarbage("collect")
local receiveBaseline=collectgarbage("count");local peak=receiveBaseline
debug.sethook(function()peak=math.max(peak,collectgarbage("count"))end,"",50000)
local ticks,maxTick=0,0
local receiveBegan=os.clock()
repeat
    local tickBegan=os.clock();assert(f.engine.Poll(f.arena,8));maxTick=math.max(maxTick,os.clock()-tickBegan);ticks=ticks+1
    assert(ticks<10000,"1 MiB validation did not terminate")
until not f.engine.HasPendingValidation()
local receiveSeconds=os.clock()-receiveBegan
debug.sethook()
assert(maxTick<0.1,"incremental receiver occupied a Poll for more than 100 ms")
local terminalMemory=collectgarbage("count")
assert(peak-receiveBaseline<32768,"1 MiB receiver exceeded bounded 32 MiB transient allocation budget")
assert(f.calls()==1 and f.engine.Snapshot().terminal.outcome=="success")
assert(f.engine.Snapshot().request.acceptedFrames==1)
collectgarbage("collect");local afterLarge=collectgarbage("count")
assert(afterLarge-baseline<256,"large command retained private payload or shadow")
for n=2,141 do
    f.command("return true",{seq=n,ack=f.ack()})
    assert(f.calls()==n and f.engine.Snapshot().terminal.outcome=="success")
end
local state=f.engine.Snapshot()
assert(f.writeStop(F.P.Kind.close,f.ack(),{seq=141,pub=1,id=state.request.requestId,digest=state.request.requestSHA256,challenge=state.request.challenge}))
collectgarbage("collect");local retained=collectgarbage("count")-baseline
assert(f.engine.Snapshot().phase=="closed" and not f.engine.Snapshot().terminal and retained<256)
print(string.format("duplex stress: 1 MiB transfer and 140 released requests retained bounded memory (after-large delta %.1f KiB, final delta %.1f KiB)",afterLarge-baseline,retained))
print(string.format("stock Lua receive memory: before %.1f KiB; sampled peak %.1f KiB; terminal before GC %.1f KiB; terminal after GC %.1f KiB; sampled receiver peak delta %.1f KiB",receiveBaseline,peak,terminalMemory,afterLarge,peak-receiveBaseline))
print(string.format("incremental stock Lua receive: %d polls; total CPU %.3f s; maximum Poll CPU %.3f ms (debug sampling enabled)",ticks,receiveSeconds,maxTick*1000))
