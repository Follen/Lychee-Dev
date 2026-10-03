local _, ns = ...

-- This view reports one fact only: this exact duplex request entered or left
-- ProbeExecution. Receiving, staging and protection no longer create UI.
local frame, activeRequest, failed, animationTime = nil, nil, false, 0
local function valid(id)
    return not (issecretvalue and issecretvalue(id)) and type(id)=="string" and #id==32 and id:match("^[0-9a-f]+$")~=nil
end
local function hide()
    if not frame then return end
    pcall(frame.SetScript,frame,"OnUpdate",nil)
    pcall(frame.Hide,frame)
end
local function reducedMotion()
    local state=ns.Persistence and ns.Persistence.Current()
    if state and state.options and state.options.reducedMotion~=nil then return state.options.reducedMotion==true end
    return ns.Theme.reducedMotion==true
end
local function failUI()
    failed=true;hide()
end
local function show(id)
    if failed then return end
    if not frame then
        local ok,created=pcall(CreateFrame,"Frame",nil,UIParent)
        if not ok or not created then failed=true;return end
        frame=created
        local okUI=pcall(function()
            frame:SetSize(120,120);frame:SetFrameStrata("DIALOG");frame:EnableMouse(false)
            frame:SetPoint("TOPLEFT",UIParent,"TOPLEFT",8,-12)
            frame.logo=frame:CreateTexture(nil,"ARTWORK");frame.logo:SetTexture(ns.Widgets.LOGO_TEXTURE)
            frame.logo:SetSize(96,96);frame.logo:SetPoint("TOP",frame,"TOP",0,0)
            frame.label=frame:CreateFontString(nil,"OVERLAY","GameFontHighlightSmall")
            ns.Theme.SetFont(frame.label,14,ns.Theme.text);frame.label:SetPoint("TOP",frame.logo,"BOTTOM",0,0)
        end)
        if not okUI then failUI();return end
    end
    activeRequest=id
    animationTime=0
    local okUI=pcall(function()
        frame.label:SetText(ns.L.ACTIVITY_PROBE)
        ns.Theme.DrawConnectionBounce(frame.logo,frame,96,0)
        frame:Show()
        if not reducedMotion() then
            frame:SetScript("OnUpdate",function(self,elapsed)
                if activeRequest~=id then return end
                local motionOK,motion=pcall(reducedMotion)
                if not motionOK then failUI();return end
                if motion then
                    local ok=pcall(ns.Theme.DrawConnectionBounce,self.logo,self,96,0)
                    pcall(self.SetScript,self,"OnUpdate",nil)
                    if not ok then failUI() end
                    return
                end
                animationTime=animationTime+(tonumber(elapsed) or 0)
                local ok=pcall(ns.Theme.DrawConnectionBounce,self.logo,self,96,animationTime)
                if not ok then failUI() end
            end)
        else frame:SetScript("OnUpdate",nil) end
    end)
    if not okUI then failUI() end
end
ns.ActivityView={
    RunStarted=function(id)if valid(id) then local ok=pcall(show,id);if not ok then failUI() end end end,
    RunFinished=function(id)if activeRequest==id then activeRequest=nil;hide() end end,
    Receiving=function()end,Begin=function()end,Collecting=function()end,Finish=function()end,
    Stop=function()activeRequest=nil;hide()end,Refresh=function()
        if not frame or not activeRequest or failed then return end
        local id=activeRequest
        local ok=pcall(function()
            if reducedMotion() then
                ns.Theme.DrawConnectionBounce(frame.logo,frame,96,0)
                frame:SetScript("OnUpdate",nil)
            else
                frame:SetScript("OnUpdate",function(self,elapsed)
                    if activeRequest~=id then return end
                    animationTime=animationTime+(tonumber(elapsed) or 0)
                    local drawOK=pcall(ns.Theme.DrawConnectionBounce,self.logo,self,96,animationTime)
                    if not drawOK then failUI() end
                end)
            end
        end)
        if not ok then failUI() end
    end,Anchor=function()end,
    Current=function()return activeRequest and "probe" or nil,activeRequest end,
}
