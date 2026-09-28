local _, ns = ...

-- Owner-approved native transport telemetry. One invisible sampler, no input
-- hooks or focus mutation. Explicit bridge-off removes all recurring work.
local frame, provider, record, last, sequence
local zero=string.rep("0",32)
local function sample()
    local identity=provider and provider()
    if not identity then record=nil;return end
    local tick=ns.Compat.MonotonicSeconds()
    if not tick then record=nil;return end
    local ready,reason=ns.Platform.ObserveInputState()
    sequence=sequence+1
    if sequence>=4294967295 then ns.InputState.Stop();return end
    identity.schema="lycheedev.input.v1"
    identity.sampleMillis=math.floor(tick*1000)
    identity.inputBlocked=ready~=true
    identity.reason=reason or ""
    local text=ns.CaptureWriter.Encode(identity,2048)
    if not text then record=nil;return end
    record=ns.MemoryProtocol.Encode(identity.runtime,identity.runtime,zero,5,1,sequence,text)
end
ns.InputState={
    Start=function(observe)
        provider=observe
        if frame and frame:GetScript("OnUpdate") then return true end
        sequence,last=sequence or 0,0
        if not frame then frame=CreateFrame("Frame") end
        frame:SetScript("OnUpdate",function(_,elapsed)
            last=last+elapsed
            if last<0.1 then return end
            last=0
            local ok=pcall(sample)
            if not ok then record=nil end
        end)
        local ok=pcall(sample)
        if not ok then record=nil end
        return true
    end,
    Stop=function()
        if frame then frame:SetScript("OnUpdate",nil) end
        provider,record=nil,nil
    end,
    Snapshot=function()return record end,
}
