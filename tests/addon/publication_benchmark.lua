-- Standalone Lua 5.1 comparison, not WoW CPU/GC or host latency evidence.
-- lua publication_benchmark.lua <baseline-addon-root> <candidate-addon-root>
local roots={assert(arg[1]),assert(arg[2])}
local runtime,zero=string.rep("1",32),string.rep("0",32)
local function load(root)
    local ns={}
    for _,file in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol"}) do
        assert(loadfile(root.."/Bridge/"..file..".lua"))("Lychee Dev",ns)
    end
    return ns
end
local namespaces={load(roots[1]),load(roots[2])}
local function input(ns,count)
    local last
    for i=1,count do
        local value={schema="lycheedev.input.v1",runtime=runtime,owner=string.rep("2",32),fence=1,nextSlot=3,
            guid="Player-1-1",build="120100.69933",character="Tester",realm="Fixture",
            sampleMillis=100000+i*1000,inputBlocked=false,reason=""}
        local text=assert(ns.CaptureWriter.Encode(value,2048))
        last=assert(ns.MemoryProtocol.Encode(runtime,runtime,zero,5,1,i,text))
    end
    return last
end
local function bind(ns,count)
    local last
    for i=1,count do
        local engine=ns.SlotProtocol.Create({runtime=runtime,build="120100.69933",product="retail",release="3.0.2",inventory=200,
            inputState="lycheedev.input.hybrid.v1",actor=function()return {character="Tester",realm="Fixture",guid="Player-1-1"}end,
            encode=ns.CaptureWriter.Encode})
        assert(engine.Describe())
        assert(engine.Receive(1,{schema="lycheedev.slot.v2",index=1,runtime=runtime,owner=string.rep("2",32),fence=1,
            nonce=string.rep("3",32),ticket=string.rep("4",32),action="bind",guid="Player-1-1",build="120100.69933"}))
        last=assert(engine.Describe()) -- Runtime's outer Describe after bound publish.
    end
    return last
end
local function measure(ns,scenario,count)
    collectgarbage("collect");collectgarbage("stop")
    local kb=collectgarbage("count")
    local start=os.clock();local record=scenario(ns,count);local elapsed=os.clock()-start
    local allocated=collectgarbage("count")-kb
    collectgarbage("restart");collectgarbage("collect")
    return elapsed,allocated,record
end
for _,test in ipairs({{"input",input,5000},{"bind",bind,1000}}) do
    local samples={{},{}}
    for repetition=1,7 do
        local records={}
        -- Alternate ordering to avoid assigning all warm CPU runs to one side.
        for position=1,2 do
            local side=repetition%2==0 and 3-position or position
            local seconds,kb,record=measure(namespaces[side],test[2],test[3])
            samples[side][#samples[side]+1]={seconds=seconds,kb=kb};records[side]=record
        end
        assert(records[1]==records[2],"benchmark result changed bytes")
    end
    for side=1,2 do
        table.sort(samples[side],function(a,b)return a.seconds<b.seconds end)
        local median=samples[side][4]
        print(string.format("%s %s count=%d n=7 medianSeconds=%.6f medianRunAllocatedKB=%.3f",test[1],side==1 and "baseline" or "candidate",test[3],median.seconds,median.kb))
    end
end
-- Count the exercised call shape independently from timing wrappers.
for side,ns in ipairs(namespaces) do
    local json,wire=ns.CaptureWriter.Encode,ns.MemoryProtocol.Encode
    local jsonCalls,wireCalls=0,0
    ns.CaptureWriter.Encode=function(...)jsonCalls=jsonCalls+1;return json(...)end
    ns.MemoryProtocol.Encode=function(...)wireCalls=wireCalls+1;return wire(...)end
    bind(ns,1)
    print(string.format("bind %s jsonCalls=%d wireCalls=%d",side==1 and "baseline" or "candidate",jsonCalls,wireCalls))
end
