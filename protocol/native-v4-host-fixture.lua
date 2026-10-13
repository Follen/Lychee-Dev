-- Explicit offline peer for formal Rust host/transport/file/journal end-to-end tests.
local root=assert(arg[1]);local ns={}
for _,p in ipairs({"CaptureWriter","NativeWire","NativeProtocol","NativeRuntime"})do assert(loadfile(root.."/addon/Bridge/"..p..".lua"))("Lychee Dev",ns)end
local challenge=0
local box={schema="lycheedev.mailbox.v2",release="3.1.2",runtime=string.rep("1",32)}
local runtime
local actor="actor"
local a={runtime=box.runtime,build="fixture",release="3.1.2",inventory=500,actor=function()return{guid=actor}end,clock=function()return 1 end,
    challenge=function()challenge=challenge+1;return tostring(challenge)end,encode=ns.CaptureWriter.Encode,compile=function(code)return loadstring(code)end,
    execute=function(fn,budget,done)local ok,value=pcall(fn);done(ok,value,{resourcesReleased=true});return nil end}
runtime=ns.NativeRuntime.Create(a);runtime.Protocol.AttachMailbox(box)
local function hex(s)return(s:gsub(".",function(c)return string.format("%02x",string.byte(c))end))end
local function map(t)local keys={}for k in pairs(t)do keys[#keys+1]=k end;table.sort(keys);local items={}for _,k in ipairs(keys)do items[#items+1]=string.format('"%s":"%s"',k,hex(t[k]))end;return"{"..table.concat(items,",").."}"end
local function emit()
    runtime.Protocol.Identity()
    io.write('{"identity":"',hex(box.identity),'","receipts":',map(box.receipts),',"bodies":',map(box.bodies),'}\n');io.flush()
end
emit()
for line in io.lines()do
    local kind,index,path=line:match("^(%w+)|(%d+)|(.+)$")
    if line=="ACTOR" then actor="foreign-actor";emit()
    elseif kind then
        index=tonumber(index)
        local chunk=assert(loadfile(path));chunk();local e=LycheeDevSlotEnvelope;LycheeDevSlotEnvelope=nil
        if kind=="OPEN"then runtime.Receive(index,e,true)
        else
            a.load=function()return runtime.Receive(index,e)end
            if kind=="EXECUTE"then runtime.Wake()elseif kind=="SETTLE"then runtime.Settle()elseif kind=="CLOSE"then runtime.Close()end
        end
        emit()
    else error("fixture command invalid")end
end
