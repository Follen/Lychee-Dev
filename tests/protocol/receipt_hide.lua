local root = assert(arg[1])
local profiles = {
    {client="Live",product="retail",version="12.1.0",interface=120100},
    {client="Pandaria",product="classic",version="5.5.4",interface=50504},
    {client="Titan",product="titan",version="3.80.2",interface=38002},
    {client="Evergreen",product="forever",version="1.60.1",interface=16001},
}
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
local nonce = "0123456789abcdef0123456789abcdef"
local realPrint = print
for _, profile in ipairs(profiles) do
    local frames, callbacks = {}, {}
    local character, guid, focus = "Paladin", "Player-1-123", {}
    GetBuildInfo = function() return profile.version, "12345", "date", profile.interface end
    UnitFullName = function() return character, "Realm" end
    UnitGUID = function() return guid end
    IsLoggedIn = function() return true end
    InCombatLockdown = function() return false end
    GetCurrentKeyBoardFocus = function() return focus end
    LycheeToolkitDB = nil
    local scale = 1
    UIParent = { GetEffectiveScale = function() return scale end }
    CreateFrame = function(kind, name, parent)
        assert(kind == "Frame" and name == nil and parent == UIParent)
        local frame = { textures = {}, events = {} }
        function frame:Hide() self.visible = false end
        function frame:Show() self.visible = true end
        function frame:EnableMouse(value) assert(value == false) end
        function frame:SetFrameStrata(value) assert(value == "DIALOG") end
        function frame:SetPoint(...) self.point = {...} end
        function frame:SetSize(w, h) self.width, self.height = w, h end
        function frame:UnregisterAllEvents() self.events = {} end
        function frame:RegisterEvent(event)
            assert(event == "PLAYER_LEAVING_WORLD" or event == "PLAYER_REGEN_DISABLED" or event == "LOADING_SCREEN_ENABLED")
            self.events[event] = true
        end
        function frame:SetScript(event, fn) assert(event == "OnEvent"); self.callback = fn end
        function frame:CreateTexture(_, layer)
            local texture = { layer = layer }
            function texture:SetColorTexture(r, g, b, a) self.black = r == 0 end
            function texture:SetAllPoints(target) assert(target == frame) end
            function texture:ClearAllPoints() end
            function texture:SetPoint(anchor, target, relative, x, y)
                assert(anchor == "TOPLEFT" and target == frame and relative == "TOPLEFT")
                self.x, self.y = x, -y
            end
            function texture:SetSize(w, h) self.width, self.height = w, h end
            function texture:Show() self.visible = true end
            function texture:Hide() self.visible = false end
            self.textures[#self.textures + 1] = texture
            return texture
        end
        frames[#frames + 1] = frame
        return frame
    end
    EventRegistry = {
        RegisterCallback = function(_, event, callback, owner)
            assert(event == "ChatFrame.OnEditBoxFocusGained" or event == "ChatFrame.OnEditBoxFocusLost")
            assert(callbacks[owner] == nil)
            callbacks[owner] = { event = event, callback = callback }
        end,
        UnregisterCallback = function(_, event, owner)
            assert(callbacks[owner] and callbacks[owner].event == event)
            callbacks[owner] = nil
        end,
    }
    local printed = {}
    print = function(message) printed[#printed + 1] = message end
    local ns = { Release = "2.0.2", Startup = { ready = true, identity = {} } }
    assert(loadfile(root .. "/Clients/" .. profile.client .. ".lua"))("Lychee Dev", ns)
    for _, name in ipairs({ "Core/Platform.lua", "Core/Persistence.lua", "Bridge/CaptureWriter.lua",
        "Bridge/Session.lua", "Bridge/MatrixSymbol.lua", "Bridge/ReceiptView.lua",
        "Core/Controls.lua" }) do
        assert(loadfile(root .. "/" .. name))("Lychee Dev", ns)
    end
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = {} }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    assert(loadfile(root .. "/Bridge/Reentry.lua"))("Lychee Dev", ns)
    assert(loadfile(root .. "/Bridge/Identity.lua"))("Lychee Dev", ns)
    assert(ns.Persistence.Load())
    -- Dismissal without a displayed card stays silent and succeeds.
    assert(ns.Controls.Handle("bridge hide") == true and #frames == 0 and #printed == 0)
    -- Show a real identity receipt, then dismiss it through the command path.
    local receipt = assert(ns.Identity.Trigger(nonce))
    assert(ns.ReceiptView.ShowIdentity(receipt))
    assert(#frames == 1 and frames[1].visible)
    assert(ns.Controls.Handle("bridge hide") == true)
    assert(not frames[1].visible and #printed == 0)
    -- Idempotent: a second dismissal neither fails nor allocates.
    assert(ns.Controls.Handle("BRIDGE HIDE") == true and #frames == 1 and not frames[1].visible)
    -- Re-show a card so the busy refusals below have visible state to keep.
    assert(ns.ReceiptView.ShowIdentity(assert(ns.Identity.Refresh(nonce))))
    assert(frames[1].visible)
    -- A pending reentry owns the display; dismissal fails closed.
    LycheeToolkitDB.reentry = { schema = "lycheedev.reentry.v1" }
    local refused, reason = ns.Controls.Handle("bridge hide")
    assert(refused == nil and reason == "receipt_busy" and frames[1].visible, tostring(reason))
    LycheeToolkitDB.reentry = nil
    -- A busy queue entry refuses the same way, keeping the card visible.
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = { ["Busy-A"] = {
        release=ns.Release, product=profile.product, build=profile.version..".12345",
        character=character, realm="Realm", guid=guid,
        sessionNonce=string.rep("a",32), reloadNonce=string.rep("b",32),
        code="return 1", codeBytes=8, codeSHA256=string.rep("c",64),
        codeAdler32=ns.CaptureWriter.DigestBytes("return 1"),
    } } }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    refused, reason = ns.Controls.Handle("bridge hide")
    assert(refused == nil and reason == "receipt_busy" and frames[1].visible, tostring(reason))
    assert(ns.ReceiptView.Dismiss() == nil and frames[1].visible)
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = {} }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    assert(ns.Controls.Handle("bridge hide") == true and not frames[1].visible)
    -- The fixed command vocabulary rejects anything beyond bare dismissal.
    refused, reason = ns.Controls.Handle("bridge hide now")
    assert(refused == nil and reason == "usage: /dev status | connect | disconnect", tostring(reason))
    assert(#printed == 0)
end
realPrint("receipt hide: four profiles passed")
