local root,output=assert(arg[1]),assert(arg[2])
local ns={}
for _,file in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol"}) do
    assert(loadfile(root.."/Bridge/"..file..".lua"))("Lychee Dev",ns)
end
local function write(name,record)
    local file=assert(io.open(output.."/"..name..".bin","wb"));file:write(assert(record));file:close()
end
local zero,runtime=string.rep("0",32),string.rep("1",32)
for kind=1,5 do
    for _,seq in ipairs({0,1,4294967295}) do
        local payload="荔枝\0\255\n\\\""..string.char(kind)
        write("wire-"..kind.."-"..string.format("%.0f",seq),ns.MemoryProtocol.Encode(kind==5 and runtime or zero,runtime,zero,kind,255,seq,payload))
    end
end
write("maximum",ns.MemoryProtocol.Encode(runtime,runtime,zero,3,3,7,string.rep("x",524288)))
local actor={character="荔枝\"\n\0",realm="服\\",guid="Player-1-1"}
local engine=ns.SlotProtocol.Create({runtime=runtime,build="120100.69933",product="retail",release="3.0.2",inventory=200,
    inputState="lycheedev.input.hybrid.v1",actor=function()return actor end,encode=ns.CaptureWriter.Encode})
write("identity-first",engine.Describe());write("identity-same",engine.Describe())
local bind={schema="lycheedev.slot.v2",index=1,runtime=runtime,owner=string.rep("2",32),fence=1,
    nonce=string.rep("3",32),ticket=string.rep("4",32),action="bind",guid=actor.guid,build="120100.69933"}
write("bound",engine.Receive(1,bind))
write("identity-inside-bind",engine.Snapshot().descriptor);write("identity-after-bind",engine.Describe())
engine.Receive(2,nil);write("identity-empty",engine.Describe())
actor.character="Relogin";write("identity-actor",engine.Describe())
