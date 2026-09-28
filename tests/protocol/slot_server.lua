-- Persistent Lua peer for Go host crash/recovery tests. Binary records are
-- retained on disk to model stale Lua heap strings; no stdout binary framing.
local root,out=assert(arg[1]),assert(arg[2])
local ns={}
for _,name in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol"}) do
    assert(loadfile(root.."/Bridge/"..name..".lua"))("Lychee Dev",ns)
end
local executions=0
local engine=ns.SlotProtocol.Create({runtime=string.rep(arg[3] or "1",32),build="70000",product="retail",release="2.5.1",
    actor=function()return {guid="Player-1-123",character="Tester",realm="Realm"}end,
    encode=ns.CaptureWriter.Encode,compile=loadstring,
    execute=function(fn,_,done)
        executions=executions+1
        local ok,result=pcall(fn)
        done(ok,result,{resourcesReleased=arg[4]~="cleanup_error"})
    end})
local seen,n={},0
local function persist(record)
    if not record or seen[record] then return end
    seen[record]=true;n=n+1
    local file=assert(io.open(out.."/record-"..n..".bin","wb"));file:write(record);file:close()
end
io.stdout:setvbuf("no")
print("ready")
for input in io.lines() do
    if input=="quit" then break end
    local index,path=input:match("^(%d+)|(.+)$")
    assert(index and path)
    assert(loadfile(path))()
    local e=LycheeDevSlotEnvelope;LycheeDevSlotEnvelope=nil
    engine.Receive(tonumber(index),e)
    local snapshot=engine.Snapshot()
    for _,record in pairs(snapshot.receipts)do persist(record)end
    for _,op in pairs(snapshot.operations)do persist(op.body)end
    local count=assert(io.open(out.."/executions.txt","w"));count:write(executions);count:close()
    print("done")
end
