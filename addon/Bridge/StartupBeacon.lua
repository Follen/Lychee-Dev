local _, ns = ...

-- A short-lived visual hint, never an identity or execution permission. It is
-- armed only for an opted-in bridge at startup and stops permanently on wake.
local frame, blocks, expiry, pulse, untilTime
local entered, loadingDone, phase, stopped
local colors = {{1,0,0},{0,1,0},{0,0,1}}
local function stop()
    stopped = true
    if expiry then expiry:Cancel(); expiry=nil end
    if pulse then pulse:Cancel(); pulse=nil end
    if frame then frame:Hide(); frame:UnregisterAllEvents(); frame:SetScript("OnEvent",nil) end
end
local function paint()
    for index=1,3 do blocks[index]:SetColorTexture(unpack(colors[(index+phase-1)%3+1])) end
end
local function tick()
    pulse=nil
    if stopped or GetTime()>=untilTime then stop(); return end
    phase=(phase+1)%3
    paint()
    pulse=C_Timer.NewTimer(.5,tick)
end
local function showReady()
    if stopped or not entered or not loadingDone or frame:IsShown() then return end
    if GetTime()>=untilTime then stop(); return end
    if not ns.ReceiverBindings.Current() or ns.Platform.ObserveInputState()~=true then return end
    phase=0; paint(); frame:Show()
    pulse=C_Timer.NewTimer(.5,tick)
end
ns.StartupBeacon = {
    Stop=stop,
    Arm=function()
        if frame or not ns.Persistence.BridgeEnabled() then return end
        if type(C_Timer)~="table" or type(C_Timer.NewTimer)~="function" then return end
        frame=CreateFrame("Frame",nil,UIParent)
        frame:Hide(); frame:EnableMouse(false); frame:SetFrameStrata("TOOLTIP")
        local px=ns.Compat.GetPhysicalPixelSize()
        frame:SetPoint("TOPLEFT",UIParent,"TOPLEFT",4*px,-4*px)
        frame:SetSize(32*px,12*px)
        local background=frame:CreateTexture(nil,"BACKGROUND")
        background:SetAllPoints(frame); background:SetColorTexture(0,0,0,1)
        blocks={}
        for index=1,3 do
            local block=frame:CreateTexture(nil,"ARTWORK")
            block:SetSize(8*px,8*px)
            block:SetPoint("TOPLEFT",(2+(index-1)*10)*px,-2*px)
            blocks[index]=block
        end
        stopped=false; untilTime=GetTime()+45
        expiry=C_Timer.NewTimer(45,stop)
        for _,event in ipairs({"PLAYER_ENTERING_WORLD","LOADING_SCREEN_DISABLED","LOADING_SCREEN_ENABLED",
            "PLAYER_REGEN_ENABLED","PLAYER_REGEN_DISABLED","PLAYER_LEAVING_WORLD"}) do frame:RegisterEvent(event) end
        frame:SetScript("OnEvent",function(_,event,initial,reloading)
            if event=="PLAYER_LEAVING_WORLD" or event=="PLAYER_REGEN_DISABLED" then stop(); return end
            if event=="PLAYER_ENTERING_WORLD" then
                if (issecretvalue and (issecretvalue(initial) or issecretvalue(reloading))) or initial~=false or reloading~=true then stop(); return end
                entered=true
            elseif event=="LOADING_SCREEN_ENABLED" then
                loadingDone=false; frame:Hide()
                if pulse then pulse:Cancel(); pulse=nil end
            elseif event=="LOADING_SCREEN_DISABLED" then loadingDone=true end
            showReady()
        end)
    end,
}
