local root = assert(arg[1])

local function scenario(fault)
    local frames, timers, fonts = {}, {}, 0
    UIParent = {}
    issecretvalue = function() return false end
    InCombatLockdown = function() return false end
    GetTime = function()
        if fault == "before_timer" then error("injected clock failure") end
        return 0
    end
    C_Timer = { NewTimer = function(_, callback)
        if fault == "before_timer" then error("injected timer API failure") end
        local timer = { callback = callback }
        function timer:Cancel() self.cancelled = true end
        timers[#timers + 1] = timer
        return timer
    end }
    local function object(kind)
        local value = { kind = kind, visible = true, scripts = {}, events = {}, text = "" }
        function value:SetScript(name, handler) self.scripts[name] = handler end
        function value:GetScript(name) return self.scripts[name] end
        function value:Hide() self.visible = false end
        function value:Show() self.visible = true end
        function value:IsShown() return self.visible end
        function value:GetFrameLevel() return 1 end
        function value:RegisterEvent(event) self.events[event] = true end
        function value:UnregisterAllEvents() self.events = {} end
        function value:SetText(text) self.text = text end
        function value:SetFocus() self.focus = true end
        function value:HasFocus() return self.focus == true end
        function value:ClearFocus() self.focus = false end
        function value:CreateTexture() return object("Texture") end
        function value:CreateFontString() return object("FontString") end
        for _, method in ipairs({"SetAllPoints", "SetFrameStrata", "SetFrameLevel", "EnableMouse",
            "EnableMouseWheel", "SetSize", "SetPoint", "SetBackdrop",
            "SetBackdropColor", "SetBackdropBorderColor", "SetColorTexture",
            "SetTexCoord", "SetVertexColor", "SetWidth", "SetAutoFocus", "SetMaxLetters",
            "SetTextColor", "SetJustifyH", "SetShadowOffset", "SetAlpha", "ClearAllPoints"}) do
            value[method] = function() end
        end
        function value:SetTexture(path)
            if fault == "constructor" then error("injected texture API failure") end
            self.texture = path
        end
        function value:SetSize(width, height) self.width, self.height = width, height end
        function value:SetFont(_, _, flags)
            if fault == "constructor" then error("injected EditBox API failure") end
            if kind == "EditBox" and flags == nil then
                error("bad argument #3 to SetFont (Usage self:SetFont(fontFile,height,flags))")
            end
        end
        frames[#frames + 1] = value
        return value
    end
    CreateFrame = object
    local ns = {
        Release = "2.5.1", Startup = {ready = true,
            identity = {product = "retail", build = "12.1.0.70000"}},
        L = {RECEIVER_TITLE = "Connection", RECEIVER_BUDGET = "%d s | %s",
            RECEIVER_READY = "Ready"},
        Theme = {window = {.05,.05,.05}, field = {.08,.08,.08},
            fieldBorder = {.4,.4,.4}, text = {.9,.9,.9}, textMuted = {.7,.7,.7},
            textDim = {.6,.6,.6}, font = "font", SetFont = function()
                fonts = fonts + 1
                if fault == "constructor" and fonts == 2 then
                    error("injected FontString API failure")
                end
            end},
        Widgets = {LOGO_TEXTURE = "logo"},
        InputProtocol = {MaxBytes = 256},
        ReceiverBindings = {
            Current = function() return {wake="ALT-CTRL-]",submit="ALT-CTRL-SHIFT-]",close="ALT-CTRL-["} end,
            Display = function() return "Ctrl+Alt+[" end,
            MatchKey = function() return false end,
        },
        Persistence = {Current = function() return {} end, Bridge = function() return {} end},
        Platform = {ObserveActor = function()
            return {character="Tester",realm="Realm",guid="Player-1-1"}
        end},
        Session = {Current = function() return nil end},
        CaptureWriter = {EncodeSignal = function() return "ready" end},
        ReceiptView = {AnchorCompanion = function() return true end, ShowIdentity = function()
                if fault == "after_show" then error("injected receipt API failure") end
                return true
            end,
            Current = function() return nil end, Hide = function() end},
    }
    assert(loadfile(root .. "/UI/Theme.lua"))("Lychee Dev", ns)
    local originalFont = ns.Theme.SetFont
    ns.Theme.SetFont = function(...)
        fonts = fonts + 1
        if fault == "constructor" and fonts == 2 then
            error("injected FontString API failure")
        end
        return originalFont(...)
    end
    assert(loadfile(root .. "/Bridge/Receiver.lua"))("Lychee Dev", ns)
    local ok, result = pcall(ns.Receiver.Wake)
    if fault == "real_editbox_font" then
        assert(ok and result == true,
            "actual EditBox:SetFont(flags) contract rejected the ready receiver")
        assert(frames[1]:IsShown() and #timers == 1,
            "ready receiver lacked a visible frame and deadline")
        local rounded = 0
        for _, item in ipairs(frames) do
            if item.texture == "Interface\\AddOns\\Lychee Dev\\Media\\RoundedPanel.png" then
                rounded = rounded + 1
            end
        end
        assert(rounded == 0, "receiver regained a painted panel")
        assert(ns.Receiver.Close())
        assert(not frames[1]:IsShown() and timers[1].cancelled,
            "closing ready receiver retained its frame or deadline")
        return
    end
    assert(ok and result == nil, fault .. ": Wake leaked a Lua error")
    assert(#frames > 0, fault .. ": fault did not reach receiver UI")
    assert(not frames[1]:IsShown(), fault .. ": full-screen receiver remained visible")
    assert(not ns.Receiver.IsActive(), fault .. ": receiver remained active")
    assert(frames[1]:GetScript("OnUpdate") == nil,
        fault .. ": receiver retained OnUpdate")
    assert(next(frames[1].events) == nil, fault .. ": receiver retained events")
    for _, item in ipairs(frames) do
        if item.kind == "EditBox" then
            assert(not item:HasFocus(), fault .. ": editor retained focus")
        end
    end
    for _, timer in ipairs(timers) do
        assert(timer.cancelled, fault .. ": deadline remained armed")
    end
    assert(ns.Receiver.Close(), fault .. ": partial Close failed")
    assert(not frames[1]:IsShown(), fault .. ": Close reopened the blocker")
end

scenario("real_editbox_font")
scenario("constructor")
scenario("before_timer")
scenario("after_show")
print("receiver rollback ok")
