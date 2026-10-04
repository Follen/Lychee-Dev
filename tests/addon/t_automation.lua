-- Read-only duplex projection and workbench rendering tests.
local frameCount = 0

local function NewRegion(name)
    local region = {
        name = name,
        shown = true,
        text = "",
        width = 0,
        height = 0,
        verticalScroll = 0,
        minimumValue = 0,
        maximumValue = 0,
        value = 0,
    }

    function region:GetName()
        return self.name
    end

    function region:SetSize(width, height)
        self.width = width
        self.height = height
    end

    function region:SetWidth(width)
        self.width = width
    end

    function region:SetHeight(height)
        self.height = height
    end

    function region:GetWidth()
        return self.width
    end

    function region:GetHeight()
        return self.height
    end

    function region:SetPoint(...)
        self.point = { ... }
    end

    function region:ClearAllPoints()
        self.point = nil
    end

    function region:SetText(text)
        self.text = text or ""
    end

    function region:GetText()
        return self.text
    end

    function region:SetFocus()
        self.focused = true
    end

    function region:ClearFocus()
        self.focused = false
    end

    function region:HighlightText()
        self.highlighted = true
    end

    function region:SelectAll()
        self.focused = true
        self.highlighted = true
    end

    function region:GetStringHeight()
        return 14
    end

    function region:GetStringWidth()
        return #self.text * 7
    end

    function region:SetTexture(path)
        self.texture = path
    end

    function region:SetScript(scriptName, handler)
        local scripts = rawget(self, "scripts")
        if not scripts then
            scripts = {}
            rawset(self, "scripts", scripts)
        end
        scripts[scriptName] = handler
    end

    function region:Hide()
        local wasShown = self.shown
        self.shown = false
        local scripts = rawget(self, "scripts")
        if wasShown and scripts and scripts.OnHide then
            scripts.OnHide(self)
        end
    end

    function region:Show()
        local wasShown = self.shown
        self.shown = true
        local scripts = rawget(self, "scripts")
        if not wasShown and scripts and scripts.OnShow then
            scripts.OnShow(self)
        end
    end

    function region:SetShown(shown)
        if shown then
            self:Show()
        else
            self:Hide()
        end
    end

    function region:IsShown()
        return self.shown
    end

    function region:GetVerticalScroll()
        return self.verticalScroll
    end

    function region:SetVerticalScroll(offset)
        self.verticalScroll = offset
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnVerticalScroll then
            scripts.OnVerticalScroll(self, offset)
        end
    end

    function region:SetScrollChild(child)
        self.scrollChild = child
    end

    function region:GetScrollChild()
        return self.scrollChild
    end

    function region:GetVerticalScrollRange()
        local childHeight = self.scrollChild and self.scrollChild:GetHeight() or 0
        return math.max(0, childHeight - (self:GetHeight() or 0))
    end

    function region:SetMinMaxValues(minimum, maximum)
        self.minimumValue = minimum
        self.maximumValue = maximum
    end

    function region:GetMinMaxValues()
        return self.minimumValue, self.maximumValue
    end

    function region:SetValue(value)
        self.value = value
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnValueChanged then
            scripts.OnValueChanged(self, value)
        end
    end

    function region:GetValue()
        return self.value
    end

    function region:SetEnabled(enabled)
        self.enabled = enabled and true or false
    end

    function region:IsEnabled()
        return self.enabled ~= false
    end

    function region:Click()
        if not self:IsEnabled() then
            return
        end
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnClick then
            scripts.OnClick(self, "LeftButton")
        end
    end

    function region:GetFrameLevel()
        return 1
    end

    function region:CreateTexture()
        return NewRegion()
    end

    function region:CreateFontString()
        return NewRegion()
    end

    setmetatable(region, {
        __index = function(target, key)
            if type(key) ~= "string" or not key:match("^%u") then return nil end
            local noOp = function()
            end
            rawset(target, key, noOp)
            return noOp
        end,
    })
    return region
end

function CreateFrame(frameType, name, _, template)
    frameCount = frameCount + 1
    local frame = NewRegion(name)
    frame.frameType = frameType
    frame.hasBackdrop = template == "BackdropTemplate"
    if not frame.hasBackdrop then
        frame.SetBackdrop = false
        frame.SetBackdropColor = false
        frame.SetBackdropBorderColor = false
    end
    return frame
end

function wipe(target)
    for key in pairs(target) do
        target[key] = nil
    end
end

function issecretvalue()
    return false
end

function time()
    return 1234567890
end

function date(_, timestamp)
    return tostring(timestamp)
end

function GetLocale()
    return "enUS"
end

function InCombatLockdown()
    return false
end

ChatFontNormal = {}
GameFontNormal = {}
GameFontNormalLarge = {}
GameFontHighlightSmall = {}
GameFontDisableSmall = {}
C_Timer = {
    After = function(_, callback)
        callback()
    end,
}

local function LoadFile(relativePath)
    local prefixes = { "addon/", "../../addon/", "../addon/" }
    for index = 1, #prefixes do
        local chunk = loadfile(prefixes[index] .. relativePath)
        if chunk then
            return chunk
        end
    end
    error("cannot load addon file: " .. relativePath)
end

local function LoadAddonFile(relativePath, namespace)
    return LoadFile(relativePath)("Lychee Dev", namespace)
end

-- Current mailbox projection: the page cannot execute or acknowledge work.
local ns={}
LoadAddonFile("Core/Locale.lua",ns)
LoadAddonFile("Core/Locale_enUS.lua",ns)
LoadAddonFile("UI/Theme.lua",ns)
LoadAddonFile("UI/Widgets.lua",ns)
LoadAddonFile("Bridge/CaptureWriter.lua",ns)
local state={phase="ready_unbound",ready=true,actorReady=true,transportReady=true,controlReady=true}
ns.DuplexRuntime={Snapshot=function()return {enabled=true,protocol=state}end}
ns.Safety={IsCombatBlocked=function()return false end,PrintBlocked=function()end}
LoadAddonFile("Modules/AutomationView.lua",ns)
local view=ns.AutomationView
assert(view.Collect()==0)
local id=string.rep("a",32)
state={phase="running",request={requestId=id,totalBytes=1048576,requestSHA256=string.rep("b",64),requestSeq="9007199254740993"}}
assert(view.Collect()==1 and view.GetRecord(id).status=="running")
assert(not view.Execute and not view.ShowNotice,"retired execution/QR API returned")
assert(view.ClearRecords()==0,"unsettled request was hidden")
LoadAddonFile("UI/Pages/Automation.lua",ns)
local page=ns.CreateAutomationPage(NewRegion("parent"))
assert(not page.executeButton and not page.showNoticeButton and not page.hideNoticeButton)
assert(page.rows[1].requestId==id)
assert(page.mailboxText:GetText()==ns.L.AUTO_MAILBOX_BUSY)
page:Activate()
assert(page.statusValue:GetText()==ns.L.AUTO_STATUS_RUNNING)
state={phase="validating",validation={requestId=id,totalBytes=1048576,requestSHA256=string.rep("b",64),copiedBytes=8192,hashedBytes=0},terminal={requestId=string.rep("e",32),outcome="failed"}}
page:Refresh()
assert(page.statusValue:GetText()==ns.L.AUTO_STATUS_QUEUED and page.mailboxText:GetText()==ns.L.AUTO_MAILBOX_BUSY)
assert(view.GetRecord(id).terminal==nil,"previous result was attributed to validating command")
state={phase="running",request={requestId=id,totalBytes=1048576,requestSHA256=string.rep("b",64),requestSeq="9007199254740993"}}
local changes=0
view.SetChangeHandler(function()changes=changes+1 end)
view.Changed();assert(changes==1)
view.SetChangeHandler(function()error("cosmetic failure")end)
assert(pcall(view.Changed))
view.SetChangeHandler(nil)
state.phase="result_pending";state.terminal={outcome="success",resultSHA256=string.rep("c",64),resultBytes=4,pages=1}
view.Collect()
assert(view.GetRecord(id).probeStatus=="completed")
assert(view.GetRecord(id).status=="reported")
assert(view.GetReportText(id):find('"resultBytes":4',1,true))
assert(view.ClearRecords()==0)
state={phase="ready_unbound",ready=true,actorReady=true,transportReady=true,controlReady=true,released={requestId=id,requestSHA256=string.rep("b",64)}}
view.Collect();assert(view.GetRecord(id).status=="acknowledged")
assert(view.GetReportText(id)==nil,"released view retained result manifest")
assert(view.ClearRecords()==1 and view.Collect()==0)
page:Refresh();assert(page.mailboxText:GetText()==ns.L.AUTO_MAILBOX_READY)
for i=1,140 do
    local nextID=string.format("%032x",i)
    state={phase="running",request={requestId=nextID,totalBytes=1048576}}
    view.Collect()
    assert(view.GetCount()==1 and #view.GetOrder()==1)
    assert(view.GetRecord(id)==nil,"view retained preceding requests")
end
assert(not view.GetRecord(view.GetOrder()[1]).reportBody)
page:Hide()
print("duplex read-only workbench, current projection and bounded retention ok")
