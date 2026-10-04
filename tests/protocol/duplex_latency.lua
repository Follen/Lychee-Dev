-- Offline addon receive/validation timing. Host encoding and row publication
-- are prepared before the timed Poll. Stock Lua has the pure-Lua SHA fallback.
local F=assert(loadfile(arg[1].."/tests/protocol/duplex_fixture.lua"))()
io.stdout:setvbuf("no")
local began,admitted
local f=F.engine({compile=function(source)
    admitted=os.clock()-began
    return loadstring(source,"=mailbox_latency")
end})
if arg[2]=="split" then
    local source="return true"..string.rep(" ",1048576-11)
    local wire=F.header(F.P.Kind.frame,source,{challenge=f.engine.Snapshot().readyChallenge})
    local h=assert(F.P.DecodeHeader(wire:sub(1,320)))
    F.setRow(f.arena.command,wire);wire=nil;source=nil;collectgarbage("collect")
    local before=os.clock();source=assert(F.P.ReadPayload(f.arena.command,1048576,1048576));local copy=os.clock()-before
    before=os.clock();assert(F.P.RequestDigest(h.requestId,h.actorBinding,h.budget,h.utcHi,h.utcLo,h.totalBytes,source)==h.requestSHA);local sha=os.clock()-before
    before=os.clock();assert(loadstring(source,"=mailbox_latency_compile"));local compile=os.clock()-before
    local empty=assert(F.P.NewArena(F.identity.arenaGeneration));local words=empty.command
    local function safe(value)return not (issecretvalue and issecretvalue(value))end
    before=os.clock()
    for n=84,262224 do local value=rawget(words,n);assert(safe(value) and value==0) end
    local tail=os.clock()-before
    print(string.format("stock Lua split (%s): 1 MiB private copy %.3f ms; full-source SHA %.3f ms; compile %.3f ms; small-command zero-tail equivalent %.3f ms",bit and "native bit" or "pure fallback",copy*1000,sha*1000,compile*1000,tail*1000))
    return
end
for n=1,5 do
    local options={seq=n,challenge=f.engine.Snapshot().readyChallenge}
    if n>1 then options.ack=f.ack() end
    local wire=F.header(F.P.Kind.frame,"return true",options)
    F.setRow(f.arena.command,wire);wire=nil;collectgarbage("collect")
    began=os.clock();assert(f.engine.Poll(f.arena,8));local seconds=os.clock()-began
    assert(f.calls()==n)
    print(string.format("stock Lua small command receive+validate+admit: %.3f ms; through terminal: %.3f ms",admitted*1000,seconds*1000))
end
if arg[2]=="small" then return end
local source="return true"..string.rep(" ",1048576-11)
local wire=F.header(F.P.Kind.frame,source,{seq=6,challenge=f.engine.Snapshot().readyChallenge,ack=f.ack()})
F.setRow(f.arena.command,wire);source=nil;wire=nil;collectgarbage("collect")
began=os.clock();local ticks,maxTick=0,0
repeat
    local tickBegan=os.clock();assert(f.engine.Poll(f.arena,8));maxTick=math.max(maxTick,os.clock()-tickBegan);ticks=ticks+1
until not f.engine.HasPendingValidation()
local seconds=os.clock()-began
assert(f.calls()==6 and f.engine.Snapshot().terminal.outcome=="success")
print(string.format("stock Lua 1 MiB command receive+validate+admit: %.3f ms; through terminal: %.3f ms",admitted*1000,seconds*1000))
print(string.format("incremental 1 MiB admission: %d polls, maximum Poll CPU %.3f ms; %s",ticks,maxTick*1000,bit and "native bit" or "pure fallback"))
