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
    -- A reset with a malformed nonce never becomes an action. Uppercase hex
    -- is lowercased first (same as identify), so only non-hex text refuses.
    assert(ns.Controls.Handle("bridge reset short") == nil)
    assert(ns.Controls.Handle("bridge reset " .. string.rep("z", 32)) == nil)
    assert(#frames == 0 and #printed == 0)
    -- Idle reset is a no-op that still proves liveness with a receipt.
    local idle = assert(ns.Controls.Handle("bridge reset " .. nonce))
    assert(type(idle) == "string" and #idle > 0 and #frames == 1 and frames[1].visible)
    assert(not idle:find("identity_busy", 1, true))
    -- A busy queue entry for this character blocks identity; reset clears it.
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = { ["Busy-A"] = {
        release=ns.Release, product=profile.product, build=profile.version..".12345",
        character=character, realm="Realm", guid=guid,
        sessionNonce=string.rep("a",32), reloadNonce=string.rep("b",32),
        code="return 1", codeBytes=8, codeSHA256=string.rep("c",64),
        codeAdler32=ns.CaptureWriter.DigestBytes("return 1"),
    } } }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    assert(ns.Identity.Trigger(nonce) == nil) -- identity_busy fail closed
    -- A stale reentry ticket is discarded by the reset; malformed tickets
    -- stay fail-closed by design.
    LycheeToolkitDB.reentry = { schema = "lycheedev.reentry.v1", standalone = true,
        requestId = "RELOAD-" .. string.rep("d", 32), sessionNonce = string.rep("a", 32),
        reloadNonce = string.rep("e", 32), runtimeEpoch = 1, character = character,
        realm = "Realm", guid = guid, release = ns.Release, product = profile.product,
        build = profile.version .. ".12345" }
    assert(ns.Controls.Handle("bridge reset " .. nonce))
    assert(not LycheeToolkitDB.reentry)
    assert(ns.Identity.Trigger(nonce), "identity must work after reset")
    -- The reset tombstones only this character's entry; another character's
    -- entry stays pending for its own window.
    ns.ProbeDefinitions = { schema = "lycheedev.queue.v1", entries = { ["Busy-A"] = {
        release=ns.Release, product=profile.product, build=profile.version..".12345",
        character=character, realm="Realm", guid=guid,
        sessionNonce=string.rep("a",32), reloadNonce=string.rep("b",32),
        code="return 1", codeBytes=8, codeSHA256=string.rep("c",64),
        codeAdler32=ns.CaptureWriter.DigestBytes("return 1"),
    }, ["Busy-B"] = {
        release=ns.Release, product=profile.product, build=profile.version..".12345",
        character="Other", realm="Realm", guid="Player-1-999",
        sessionNonce=string.rep("a",32), reloadNonce=string.rep("b",32),
        code="return 1", codeBytes=8, codeSHA256=string.rep("c",64),
        codeAdler32=ns.CaptureWriter.DigestBytes("return 1"),
    } } }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    assert(ns.Controls.Handle("bridge reset " .. nonce))
    character, guid = "Other", "Player-1-999"
    assert(ns.Identity.Trigger(nonce) == nil, "other character's entry must stay busy")
    character, guid = "Paladin", "Player-1-123"
    assert(ns.Identity.Trigger(nonce), "original character stays clear")
    assert(#printed == 0)
end
realPrint("receipt reset: four profiles passed")
