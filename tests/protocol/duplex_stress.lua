local root, largePath, opsPath, bindPath = arg[1], arg[2], arg[3], arg[4]
local ns={CaptureWriter={Encode=function()return "{}"end}}
assert(loadfile(root.."/addon/Bridge/SHA256.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/addon/Bridge/DuplexProtocol.lua"))("Lychee Dev",ns)
local function read(path,n)local f=assert(io.open(path,"rb"));local b=f:read(n);f:close();assert(#b==n);return b end
local function words(bytes,count)
    local out={};for i=1,count do local p=(i-1)*4+1;local a,b,c,d=bytes:byte(p,p+3);out[i]=(a or 0)+((b or 0)*256)+((c or 0)*65536)+((d or 0)*16777216) end;return out
end
local function setRow(row,bytes)
    for i=1,#row do row[i]=0 end
    local values=words(bytes,#row);for i=1,#values do row[i]=values[i] end
end
local identity={runtime="00112233445566778899aabbccddeeff",arenaGeneration="10112233445566778899aabbccddeeff",
    session="20112233445566778899aabbccddeeff",owner="30112233445566778899aabbccddeeff",
    actorBindingId="40112233445566778899aabbccddeeff",actorGUID="Player-1-1",character="Paladin",realm="Realm",build="12.1.0.12345",product="retail"}
local arena=assert(ns.DuplexProtocol.NewArena(identity.arenaGeneration))
local resultBody='{"ok":true,"result":true,"resourcesReleased":true}'
local pages={};local sequence=9007199254740993
local engine=assert(ns.DuplexProtocol.Create({identity=identity,publish=function()return true end,
    compile=function(source)return loadstring(source,"=duplex_stress")end,
    execute=function(fn,_,done)local ok,value=pcall(fn);done(ok,value,{resourcesReleased=true});return {RequestCancel=function()return nil,"sync" end}end,
    clock=function()return 100 end,challenge=function()return string.rep(string.char(187),16)end,
    encode=function()return resultBody end,actor=function()return {guid=identity.actorGUID,character=identity.character,realm=identity.realm}end,
    publishPage=function(index,body)pages[index]=body end,clearPages=function()pages={}end}))
assert(engine.BindIdentity(identity));assert(engine.Enable())
setRow(arena.control.bindResume,read(bindPath,320));assert(engine.Poll(arena,8))
collectgarbage("collect");local baseline=collectgarbage("count")
local f=assert(io.open(largePath,"rb"))
for i=1,256 do
    local wire=f:read(4416);assert(wire and #wire==4416,"truncated megabyte frame stream")
    setRow(arena.request.frames[i],wire);assert(engine.Poll(arena,8))
end
f:close();assert(engine.Snapshot().phase=="prepared","1 MiB request not prepared")
local ops=assert(io.open(opsPath,"rb"))
local function tiny()
    local frame=ops:read(328);assert(frame and #frame==328)
    local commit=ops:read(320);assert(commit and #commit==320)
    local ack=ops:read(412);assert(ack and #ack==412)
    setRow(arena.request.frames[1],frame);assert(engine.Poll(arena,8))
    assert(engine.Snapshot().phase=="prepared")
    setRow(arena.control.commit,commit);assert(engine.Poll(arena,8))
    assert(engine.Snapshot().terminal and engine.Snapshot().terminal.outcome=="success")
    setRow(arena.control.resultAck,ack);assert(engine.Poll(arena,8))
    assert(engine.Snapshot().request==nil)
end
-- Commit/ACK the 1 MiB request. Its frame cells are replaced by the first
-- bounded one-frame request below; no whole request remains in the arena.
local largeCommit=ops:read(320);assert(largeCommit and #largeCommit==320)
local largeAck=ops:read(412);assert(largeAck and #largeAck==412)
setRow(arena.control.commit,largeCommit);assert(engine.Poll(arena,8))
setRow(arena.control.resultAck,largeAck);assert(engine.Poll(arena,8))
local afterAck=engine.Snapshot()
assert(afterAck.request==nil,"large ACK failed: "..tostring(afterAck.lastFailure).." receipt="..tostring(afterAck.receipts.resultAck and afterAck.receipts.resultAck.state))
collectgarbage("collect");local afterLarge=collectgarbage("count")
assert(afterLarge-baseline<2048,"1 MiB transfer retained unexpected Lua memory")
for _=1,140 do tiny() end
ops:close();collectgarbage("collect")
local final=engine.Snapshot()
assert(final.request==nil and final.terminal==nil and final.released and final.receipts)
assert(afterLarge-baseline<2048 and collectgarbage("count")-baseline<2048,"repeated requests retained Lua memory")
print(string.format("duplex stress: 1 MiB transfer and 140 released requests retained bounded memory (after-large delta %.1f KiB, final delta %.1f KiB)",afterLarge-baseline,collectgarbage("count")-baseline))
