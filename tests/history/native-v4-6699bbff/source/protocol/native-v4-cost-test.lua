local root=arg[1]or"."
local ns={}
for _,path in ipairs({"CaptureWriter","NativeWire","NativeProtocol"})do assert(loadfile(root.."/addon/Bridge/"..path..".lua"))("Lychee Dev",ns)end
local n=0
local protocol=ns.NativeProtocol.Create({runtime=string.rep("1",32),build="fixture",release="3.1.2",inventory=500,actor=function()return{guid="actor"}end,
    clock=function()return 1 end,challenge=function()n=n+1;return tostring(n)end,encode=ns.CaptureWriter.Encode,compile=function(code)return assert(loadstring(code))end,
    execute=function(fn)return true,fn(),{resourcesReleased=true}end})
local function e(action,index,nonce,extra)
    local out={schema="lycheedev.slot.v4",runtime=string.rep("1",32),owner=string.rep("2",32),sessionEpoch=string.rep("3",32),fence=1,index=index,
        nonce=string.format("%032x",nonce),ticket=string.rep("4",32),requestSeq=1,digest="digest",action=action,guid="actor",build="fixture",challenge=tostring(n)}
    for k,v in pairs(extra or{})do out[k]=v end;return out
end
assert(protocol.Receive(1,e("open",1,1,{lane=0}),true))
collectgarbage("collect");local before=collectgarbage("count")
local exec=e("execute",21,2,{code="return string.rep('x',500000)",budget=30,expires=100})
local receipt=assert(protocol.Receive(21,exec,false))
collectgarbage("collect");local retained=collectgarbage("count")-before
local ceiling=tonumber(os.getenv("LYCHEEDEV_NATIVE_RETAINED_KIB")or"700")
assert(retained<ceiling,"candidate retained result bytes exceeded single-body bound: "..retained)
print(string.format("native-v4-cost: reportBytes=500000 retainedKiB=%.2f receiptBytes=%d ceilingKiB=%d",retained,#receipt,ceiling))
