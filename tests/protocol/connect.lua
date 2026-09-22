local root = assert(arg[1])
local profiles = {
    {toc="Mainline",version="12.1.0",interface=120100},
    {toc="Mists",version="5.5.4",interface=50504},
    {toc="Wrath",version="3.80.2",interface=38002},
    {toc="Forever",version="1.60.1",interface=16001},
}
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
for _, profile in ipairs(profiles) do
    local frames, callbacks, shown = {}, {}, {}
    local character, guid, focus = "Paladin", "Player-1-123", {}
    SlashCmdList, SLASH_LYCHEETOOLKIT1, LycheeToolkitDB = {}, nil, nil
    GetBuildInfo = function() return profile.version, "12345", "date", profile.interface end
    UnitFullName = function() return character, "Realm" end
    UnitGUID = function() return guid end
    IsLoggedIn = function() return true end
    InCombatLockdown = function() return false end
    GetCurrentKeyBoardFocus = function() return focus end
    CreateFrame = function()
        local frame = {events={},scripts={}}
        function frame:RegisterEvent(event) self.events[event] = true end
        function frame:UnregisterAllEvents() self.events = {} end
        function frame:SetScript(event, callback) self.scripts[event] = callback end
        frames[#frames+1] = frame
        return frame
    end
    EventRegistry = {
        RegisterCallback = function(self, event, callback, owner)
            assert(event == "ChatFrame.OnEditBoxFocusLost" and not callbacks[owner])
            callbacks[owner] = callback
        end,
        UnregisterCallback = function(self, event, owner)
            assert(event == "ChatFrame.OnEditBoxFocusLost")
            callbacks[owner] = nil
        end,
    }
    local ns = {}
    local toc = assert(io.open(root .. "/Lychee Dev_" .. profile.toc .. ".toc", "r"))
    for line in toc:lines() do
        line = line:gsub("\r", "")
        if line ~= "" and line:sub(1,1) ~= "#" then
            assert(loadfile(root .. "/" .. line:gsub("\\", "/")))("Lychee Dev", ns)
        end
    end
    toc:close()
    assert(ns.Controls.Handle("connect") == nil and LycheeToolkitDB == nil)
    frames[1].scripts.OnEvent(frames[1], "ADDON_LOADED", "Lychee Dev")
    assert(ns.Startup.ready and #frames == 1 and next(callbacks) == nil)
    assert(LycheeToolkitDB.options.bridgeEnabled == nil and ns.Session.Connect() == nil)
    -- Exercise the command/session path through actual TOCs. Optical rendering
    -- has separate pixel tests; retain exactly what this command would display.
    local hidden = 0
    ns.ReceiptView = {
        Hide=function() hidden=hidden+1 end,
        Show=function(receipt) shown[#shown+1]=receipt;return true end,
    }
    guid = nil
    assert(ns.Controls.Handle("connect") == nil)
    assert(LycheeToolkitDB.options.bridgeEnabled == nil and #shown == 0)
    guid = "Player-1-123"
    local status = assert(ns.Controls.Handle("connect"))
    assert(status.connected and status.character == character and status.realm == "Realm")
    assert(status.session == nil and status.sessionNonce == nil)
    local first = assert(ns.Session.Current())
    assert(#first.sessionNonce == 32 and first.sessionNonce:match("^[0-9a-f]+$"))
    assert(shown[1]:find('"inputReady":false', 1, true) and next(callbacks))
    local owner, callback = next(callbacks)
    focus = nil
    callback(owner)
    assert(next(callbacks) == nil and shown[#shown]:find('"inputReady":true', 1, true))
    io.write(shown[#shown], "\n")
    assert(ns.Controls.Handle("connect").connected)
    assert(ns.Session.Current().sessionNonce == first.sessionNonce, "connect rotated active identity")
    local reports = LycheeToolkitDB.reports
    reports.keep = {body="retained"}
    assert(ns.Controls.Handle("disconnect"))
    assert(ns.Session.Current() == nil and next(callbacks) == nil and hidden > 0)
    assert(LycheeToolkitDB.options.bridgeEnabled == nil and reports.keep.body == "retained")
    assert(ns.Controls.Handle("connect").connected)
    assert(ns.Session.Current().sessionNonce ~= first.sessionNonce)
    assert(ns.Controls.Handle("disconnect"))
    character = secret
    assert(ns.Controls.Handle("connect") == nil and LycheeToolkitDB.options.bridgeEnabled == nil)
    assert(#frames == 1 and next(callbacks) == nil)
end
