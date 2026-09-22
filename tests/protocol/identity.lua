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
        function frame:SetScale(value) self.scale = value end
        function frame:SetSize(w, h) self.width, self.height = w, h end
        function frame:UnregisterAllEvents() self.events = {} end
        function frame:RegisterEvent(event)
            assert(event == "PLAYER_LEAVING_WORLD" or event == "PLAYER_REGEN_DISABLED")
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
    local ns = { Release = "2.0.0", Startup = { ready = true, identity = {} } }
    -- This harness lists addon modules explicitly (the TOC entry is added by
    -- the integrator); Identity.lua is exercised through the real TOC-adjacent
    -- modules it depends on, not a mock.
    assert(loadfile(root .. "/Clients/" .. profile.client .. ".lua"))("Lychee Dev", ns)
    for _, name in ipairs({ "Core/Platform.lua", "Core/Persistence.lua", "Bridge/CaptureWriter.lua",
        "Bridge/Session.lua", "Bridge/MatrixSymbol.lua", "Bridge/ReceiptView.lua" }) do
        assert(loadfile(root .. "/" .. name))("Lychee Dev", ns)
    end
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = {} }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    assert(loadfile(root .. "/Bridge/Reentry.lua"))("Lychee Dev", ns)
    assert(loadfile(root .. "/Bridge/Identity.lua"))("Lychee Dev", ns)
    assert(ns.Persistence.Load())
    -- Zero cost until called: no frames before the first trigger.
    assert(#frames == 0 and ns.Identity.CancelWait() == nil)
    for _, bad in ipairs({ "", "abc", string.rep("a", 31), string.rep("a", 33),
        string.rep("A", 32), string.rep("z", 32), secret, 42 }) do
        local receipt, reason = ns.Identity.Trigger(bad)
        assert(receipt == nil and reason == "identity_invalid_nonce")
    end
    assert(#frames == 0)
    local function drawing()
        local frame = frames[#frames]
        local runs = {}
        for _, texture in ipairs(frame.textures) do
            if texture.black and texture.visible then
                runs[#runs + 1] = table.concat({ texture.x, texture.y, texture.width, texture.height }, ",")
            end
        end
        return { width = frame.width, height = frame.height, runs = runs }
    end
    -- The typed command always leaves keyboard focus in the chat edit box, so
    -- the first receipt reports the observed input state without guessing.
    local first = assert(ns.Identity.Trigger(nonce))
    assert(#frames == 0)
    assert(ns.ReceiptView.ShowIdentity(first))
    assert(#frames == 1 and frames[1].visible)
    local firstDrawing = drawing()
    local notified = nil
    assert(ns.Identity.WhenInputReady(function(ready, failure)
        assert(notified == nil and failure == nil)
        notified = ready and true or false
    end))
    local owner, watch
    for key, value in pairs(callbacks) do
        if value.event == "ChatFrame.OnEditBoxFocusLost" then
            assert(owner == nil)
            owner, watch = key, value.callback
        end
    end
    assert(owner, "no one-shot readiness callback")
    -- A busy request-scoped operation must not disturb the displayed receipt.
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = { ["Busy-A"] = {} } }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    local busyReceipt, busyReason = ns.Identity.Trigger(nonce)
    assert(busyReceipt == nil and busyReason == "identity_busy" and frames[1].visible)
    local refreshReceipt, refreshReason = ns.Identity.Refresh(nonce)
    assert(refreshReceipt == nil and refreshReason == "identity_busy" and frames[1].visible)
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = {} }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    LycheeToolkitDB.reentry = { schema = "lycheedev.reentry.v1" }
    busyReceipt, busyReason = ns.Identity.Trigger(nonce)
    assert(busyReceipt == nil and busyReason == "identity_busy" and frames[1].visible)
    LycheeToolkitDB.reentry = nil
    -- Focus release prompts one fresh observation; it is not itself proof.
    focus = nil
    watch(owner)
    assert(callbacks[owner] == nil and notified == true)
    local refreshed = assert(ns.Identity.Refresh(nonce))
    assert(ns.ReceiptView.ShowIdentity(refreshed))
    local refreshedDrawing = drawing()
    -- Actor states are facts: absent and restricted actors never fabricate
    -- character or realm.
    character, focus = nil, {}
    local noActor = assert(ns.Identity.Trigger(nonce))
    focus = nil
    character, guid = secret, secret
    local restricted = assert(ns.Identity.Trigger(nonce))
    character, guid = "Paladin", "Player-1-123"
    GetBuildInfo = function() return secret, "12345", "date", profile.interface end
    local buildReceipt, buildFailure = ns.Identity.Trigger(nonce)
    assert(buildReceipt == nil and buildFailure == "platform_restricted_identity")
    GetBuildInfo = function() return profile.version, "12345", "date", profile.interface end
    assert(ns.Identity.Trigger(nonce))
    ns.ReceiptView.Hide()
    assert(not frames[1].visible and next(callbacks) == nil)
    io.write(assert(ns.CaptureWriter.Encode({
        product = profile.product, build = profile.version .. ".12345",
        first = first, refreshed = refreshed, noActor = noActor, restricted = restricted,
        firstDrawing = firstDrawing, refreshedDrawing = refreshedDrawing,
    }, 65536)), "\n")
end
