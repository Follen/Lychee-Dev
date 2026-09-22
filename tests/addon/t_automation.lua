-- Automation workbench tests. Page cases ported from the legacy
-- UITests/AutomationTests automation block (row rendering over fixture
-- records, newest auto-select, 48 KB report display cap without payload
-- mutation, two-click clear that keeps protected/pending records) plus
-- bridge-machinery cases: status derivation from fixture queue/report records
-- and execute routing through the shared ProbeQueue/ProbeRunner functions
-- (call counting proves no second executor exists).

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

-- ---------------------------------------------------------------------------
-- Phase A: status derivation over fixture bridge records, and Collect()
-- discovery from the queue registry, the report store and a reentry ticket.
-- ---------------------------------------------------------------------------
local nsA = {}
LoadAddonFile("Core/Locale.lua", nsA)
LoadAddonFile("Core/Locale_enUS.lua", nsA)

local bridgeFixture = {
    ["req-queued"] = { absent = true, reloadScope = { codeBytes = 3, codeAdler32 = "deadbeef" } },
    ["req-loaded"] = { reportedReason = "probe_not_reported", verifyReason = "probe_still_retained" },
    ["req-reported"] = { readReceipt = '{"kind":"reported","sequence":7}', readBody = '{"probeStatus":"completed"}' },
    ["req-acked"] = { ackReceipt = '{"kind":"acknowledged"}' },
    ["req-cleared"] = { reportedReason = "probe_not_reported", absent = true, reloadReason = "queue_request_missing" },
    ["req-broken"] = { readReason = "report_invalid_store" },
    ["req-unbound"] = { reportedReason = "probe_not_reported", verifyReason = "session_unbound" },
}

local function FixtureOf(requestId)
    return bridgeFixture[requestId] or {}
end

nsA.ReportStore = {
    Read = function(requestId)
        local fixture = FixtureOf(requestId)
        if fixture.readReceipt then
            return fixture.readReceipt, fixture.readBody
        end
        return nil, fixture.readReason or "report_unavailable"
    end,
    Acknowledged = function(requestId)
        local fixture = FixtureOf(requestId)
        if fixture.ackReceipt then
            return fixture.ackReceipt
        end
        return nil, "report_acknowledgement_unavailable"
    end,
}
nsA.ProbeRunner = {
    Reported = function(requestId)
        local fixture = FixtureOf(requestId)
        return nil, fixture.reportedReason or "probe_not_reported"
    end,
    VerifyAbsent = function(requestId)
        local fixture = FixtureOf(requestId)
        if fixture.absent then
            return true
        end
        return nil, fixture.verifyReason or "probe_still_retained"
    end,
}
nsA.ProbeQueue = {
    ReloadScope = function(requestId)
        local fixture = FixtureOf(requestId)
        if fixture.reloadScope then
            return fixture.reloadScope
        end
        return nil, fixture.reloadReason or "queue_request_missing"
    end,
}
nsA.ProbeDefinitions = {
    schema = "lycheedev.queue.v1",
    entries = {
        ["req-queued"] = { codeBytes = 3, codeSHA256 = string.rep("a", 64), codeAdler32 = "deadbeef" },
        ["req-loaded"] = { codeBytes = 5, codeSHA256 = string.rep("b", 64), codeAdler32 = "cafebabe" },
    },
}
nsA.Persistence = {
    Current = function()
        return {
            reports = {
                ["req-reported"] = { receipt = '{"kind":"reported","sequence":7}', body = '{"probeStatus":"completed"}' },
            },
            reentry = { requestId = "req-acked" },
        }
    end,
}

LoadAddonFile("Modules/AutomationView.lua", nsA)
local viewA = nsA.AutomationView

assert(viewA.HasQueueRegistry(), "AutomationView did not capture the queue registry")

local expectedStatuses = {
    ["req-queued"] = "queued",
    ["req-loaded"] = "loaded",
    ["req-reported"] = "reported",
    ["req-acked"] = "acknowledged",
    ["req-cleared"] = "cleared",
    ["req-broken"] = "unavailable",
    ["req-unbound"] = "unavailable",
}
for requestId, expected in pairs(expectedStatuses) do
    local status, errorCode = viewA.DeriveStatus(requestId)
    assert(status == expected,
        requestId .. " derived status " .. tostring(status) .. " instead of " .. expected)
    if expected == "unavailable" then
        assert(type(errorCode) == "string" and errorCode ~= "",
            requestId .. " did not expose its bridge error code")
    end
end
local brokenStatus, brokenError = viewA.DeriveStatus("req-broken")
assert(brokenError == "report_invalid_store", "req-broken exposed the wrong bridge error code")
local unboundStatus, unboundError = viewA.DeriveStatus("req-unbound")
assert(unboundError == "session_unbound", "req-unbound exposed the wrong bridge error code")

viewA.Collect()
local orderA = viewA.GetOrder()
assert(#orderA == 4, "Collect did not discover exactly the queue/report/reentry records")
assert(orderA[1] == "req-reported", "Collect did not order the newest record first")
assert(viewA.GetRecord("req-queued").status == "queued", "queued record status was not derived")
assert(viewA.GetRecord("req-queued").codeBytes == 3, "queued record lost its code digest summary")
assert(viewA.GetRecord("req-loaded").status == "loaded", "loaded record status was not derived")
assert(viewA.GetRecord("req-reported").status == "reported", "reported record status was not derived")
assert(viewA.GetRecord("req-reported").reportBody == '{"probeStatus":"completed"}',
    "reported record did not carry its stored body")
assert(viewA.GetRecord("req-reported").probeStatus == "completed",
    "reported record did not surface probeStatus")
assert(viewA.GetRecord("req-acked").status == "acknowledged", "reentry record status was not derived")
assert(viewA.GetRecord("req-cleared") == nil, "Collect invented a record nobody reported")

print("automation bridge derivation tests passed")

-- ---------------------------------------------------------------------------
-- Phase B: page rendering over fixture records with a counting bridge.
-- ---------------------------------------------------------------------------
local nsB = {}
LoadAddonFile("Core/Locale.lua", nsB)
LoadAddonFile("Core/Locale_enUS.lua", nsB)
LoadAddonFile("UI/Widgets.lua", nsB)
local L = nsB.L

local loadCalls, dispatchCalls, runCount = 0, 0, 0
local loadFailure
local readCalls = 0
local receiptShowCalls, receiptHideCalls = 0, 0
local shownReceipt

nsB.Safety = {
    IsCombatBlocked = function()
        return false
    end,
    PrintBlocked = function()
        error("combat refusal must not trigger in this test")
    end,
}
nsB.ProbeQueue = {
    Load = function()
        loadCalls = loadCalls + 1
        if loadFailure then
            return nil, loadFailure
        end
        return '{"kind":"loaded"}'
    end,
    ReloadScope = function()
        return nil, "queue_request_missing"
    end,
}
nsB.ProbeRunner = {
    -- The only executor in the fixture: anything else that ran probe code
    -- would show up as runCount drift.
    Dispatch = function()
        dispatchCalls = dispatchCalls + 1
        runCount = runCount + 1
        return '{"kind":"reported"}'
    end,
    Reported = function()
        return nil, "probe_not_reported"
    end,
    VerifyAbsent = function()
        return true
    end,
}
nsB.ReportStore = {
    Read = function()
        readCalls = readCalls + 1
        return nil, "report_unavailable"
    end,
    Acknowledged = function()
        return nil, "report_acknowledgement_unavailable"
    end,
}
nsB.Persistence = {
    Current = function()
        return { reports = {} }
    end,
}
nsB.ReceiptView = {
    Show = function(receipt)
        receiptShowCalls = receiptShowCalls + 1
        shownReceipt = receipt
        return true
    end,
    Hide = function()
        receiptHideCalls = receiptHideCalls + 1
    end,
}

LoadAddonFile("Modules/AutomationView.lua", nsB)
LoadAddonFile("UI/Pages/Automation.lua", nsB)
local viewB = nsB.AutomationView

assert(not viewB.HasQueueRegistry(), "AutomationView invented a queue registry")
assert(type(viewB.Load) ~= "function" and type(viewB.Dispatch) ~= "function"
    and type(viewB.RunCode) ~= "function",
    "AutomationView exposes a second executor")

local longBody = string.rep("x", 60 * 1024)
viewB.Observe({
    requestId = "req-older", kind = "bug_snapshot", status = "acknowledged",
    observedAt = 1000, errorCode = "boom",
})
viewB.Observe({
    requestId = "req-newer", kind = "lua", status = "queued",
    observedAt = 2000, codeBytes = 12, codeSHA256 = string.rep("c", 64), codeAdler32 = "deadbeef",
    protected = true,
})
viewB.Observe({
    requestId = "req-long", kind = "lua", status = "reported",
    observedAt = 3000, reportBody = longBody, receipt = '{"kind":"reported"}', hasReport = true,
})

local parent = NewRegion("automationParent")
local page = nsB.CreateAutomationPage(parent)
page:Activate()

assert(#page.rows == 3, "automation page did not build one row per record")
assert(page.rows[1].title:GetText() == "req-long", "row 1 does not render the newest request id")
assert(page.rows[3].title:GetText() == "req-older", "rows are not ordered newest first")
assert(page.rows[1].meta:GetText() == "3000 | " .. L.AUTO_KIND_LUA,
    "row meta does not render time and kind")
assert(page.rows[1].status:GetText() == L.AUTO_STATUS_REPORTED,
    "row status does not render the bridge status label")

-- Newest record is auto-selected on refresh.
assert(page.requestValue:GetText() == "req-long", "newest record was not auto-selected")
assert(page.statusValue:GetText() == L.AUTO_STATUS_REPORTED, "detail status did not follow the selection")

page.SelectRecord("req-older")
assert(page.requestValue:GetText() == "req-older", "selecting a record did not refresh the detail pane")
assert(page.kindValue:GetText() == L.AUTO_KIND_BUG, "detail kind label is wrong")
assert(page.errorValue:GetText() == "boom", "detail error code is not shown")

-- 48 KB report display cap with the visible note, without touching the body.
local readsBeforeReport = readCalls
page.SelectRecord("req-long")
page.viewReportButton:Click()
local shownReport = page.reportArea.editBox:GetText()
assert(#shownReport > 48 * 1024 and #shownReport < 60 * 1024,
    "report view did not bound its displayed text at 48 KB")
assert(shownReport:sub(-#string.format(L.AUTO_REPORT_DISPLAY_LIMIT, 48))
        == string.format(L.AUTO_REPORT_DISPLAY_LIMIT, 48),
    "report view did not append the display-limit note")
assert(#viewB.GetRecord("req-long").reportBody == 60 * 1024
        and viewB.GetRecord("req-long").reportBody == longBody,
    "report view truncated the stored payload")
assert(readCalls == readsBeforeReport, "report view re-read or rewrote the stored report")

-- Notice goes through the one overlay system, read-only.
page.showNoticeButton:Click()
assert(receiptShowCalls == 1 and shownReceipt == '{"kind":"reported"}',
    "Show Notice did not reuse ns.ReceiptView with the stored receipt")
page.hideNoticeButton:Click()
assert(receiptHideCalls == 1, "Hide Notice did not call ns.ReceiptView.Hide")

-- Execute routes through the shared ProbeQueue/ProbeRunner functions only.
viewB.Observe({ requestId = "req-exec", kind = "lua", status = "queued", observedAt = 4000 })
page:Refresh()
page.SelectRecord("req-exec")
page.executeButton:Click()
assert(loadCalls == 1 and dispatchCalls == 1 and runCount == 1,
    "Execute did not route through ProbeQueue.Load + ProbeRunner.Dispatch exactly once")
assert(page.errorValue:GetText() == L.NOT_AVAILABLE, "successful execute displayed a phantom error")
page:Refresh()
assert(runCount == 1, "refreshing the page executed the probe a second time")

-- Honest failure: no invented state when the bridge context is absent.
viewB.Observe({ requestId = "req-fail", kind = "lua", status = "queued", observedAt = 5000 })
loadFailure = "session_unbound"
page:Refresh()
page.SelectRecord("req-fail")
page.executeButton:Click()
assert(loadCalls == 2 and dispatchCalls == 1 and runCount == 1,
    "a failed load still dispatched the probe")
assert(page.errorValue:GetText() == "session_unbound",
    "execute failure did not surface the real bridge error code")
loadFailure = nil

-- Two-click clear keeps protected and pending records.
page.clearButton:Click()
assert(page.clearButton.variant == "danger",
    "first clear click did not switch to the confirmation state")
assert(viewB.GetRecord("req-older") ~= nil, "first clear click already removed records")
page.clearButton:Click()
assert(viewB.GetRecord("req-older") == nil, "clear did not remove an ordinary record")
assert(viewB.GetRecord("req-exec") == nil, "clear did not remove an ordinary record")
assert(viewB.GetRecord("req-fail") == nil, "clear did not remove an ordinary record")
assert(viewB.GetRecord("req-newer") ~= nil, "clear removed a protected record")
assert(viewB.GetRecord("req-long") ~= nil, "clear removed a record with a pending report")

print("automation page tests passed")
