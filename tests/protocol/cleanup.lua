local root = assert(arg[1])
local products = {
    { "Mainline", "12.1.0", 120100 }, { "Mists", "5.5.4", 50504 },
    { "Wrath", "3.80.2", 38002 }, { "Forever", "1.60.1", 16001 },
}
local outputs = {}
for _, profile in ipairs(products) do
    local ns, frames = {}, {}
    LycheeToolkitDB, SlashCmdList, SLASH_LYCHEETOOLKIT1 = nil, {}, nil
    local secret = {}
    issecretvalue = function(value) return rawequal(value, secret) end
    CreateFrame = function()
        local frame = {}
        function frame:RegisterEvent() end
        function frame:UnregisterAllEvents() end
        function frame:SetScript(_, callback) self.callback = callback end
        frames[#frames + 1] = frame
        return frame
    end
    GetBuildInfo = function() return profile[2], "12345", "date", profile[3] end
    UnitFullName = function() return "Paladin", "Realm" end
    UnitGUID = function() return "Player-1-123" end
    local toc = assert(io.open(root .. "/Lychee Dev.toc"))
    for line in toc:lines() do
        line = line:gsub("\r", "")
        if line ~= "" and line:sub(1, 1) ~= "#" then
            assert(loadfile(root .. "/" .. line:gsub("\\", "/")))("Lychee Dev", ns)
        end
    end
    toc:close()
    frames[1].callback(frames[1], "ADDON_LOADED", "Lychee Dev")
    local nonce, cleanup = string.rep("a", 32), string.rep("c", 32)
    local command = "bridge verify OP-target " .. cleanup
    local shown
    ns.ReceiptView = { Hide = function() shown = nil end, Show = function(value) shown = value; return true end }
    local function rejects(method, expected)
        local value, reason = method()
        assert(value == nil and reason == expected, tostring(reason) .. " expected " .. expected)
        assert(shown == nil)
    end
    rejects(function() return ns.Controls.Handle(command) end, "bridge_disabled")
    assert(ns.Controls.Handle("bridge on"))
    rejects(function() return ns.Controls.Handle(command) end, "session_unbound")
    assert(ns.Session.Bind(nonce))
    rejects(function() return ns.ProbeQueue.VerifyRetired("OP-target", secret) end, "queue_invalid_cleanup_nonce")
    rejects(function() return ns.ProbeQueue.VerifyRetired(secret, cleanup) end, "queue_invalid_request")
    assert(ns.ProbeRunner.Load("OP-target", "return 42"))
    rejects(function() return ns.Controls.Handle(command) end, "probe_still_retained")
    assert(ns.ProbeRunner.Dispatch("OP-target"))
    rejects(function() return ns.Controls.Handle(command) end, "report_still_retained")
    -- Model a persisted post-ACK reload: rebuild runtime modules with the
    -- report removed. This is a Lua lifecycle fixture, not a running client.
    LycheeToolkitDB.reports["OP-target"] = nil
    rejects(function() return ns.Controls.Handle(command) end, "probe_still_retained")
    assert(loadfile(root .. "/Bridge/ProbeRunner.lua"))("Lychee Dev", ns)
    local foreign = { receipt = "unchanged", body = "unchanged" }
    LycheeToolkitDB.reports["OP-foreign"] = foreign
    local receipt = assert(ns.Controls.Handle(command))
    assert(shown == receipt and LycheeToolkitDB.reports["OP-foreign"] == foreign)
    assert(#frames == 1, "cleanup allocated runtime machinery")
    outputs[#outputs + 1] = receipt
    local sequence = ns.Session.Current().sequence
    assert(ns.Controls.Handle(command .. " extra") == nil)
    assert(ns.Controls.Handle("bridge verify OP-target " .. string.rep("F", 32)) == nil)
    assert(ns.Session.Current().sequence == sequence)
    ns.ProbeDefinitions = { schema = "broken", entries = {} }
    assert(loadfile(root .. "/Bridge/ProbeQueue.lua"))("Lychee Dev", ns)
    rejects(function() return ns.Controls.Handle(command) end, "queue_invalid_format")
end
io.write("[")
for index, receipt in ipairs(outputs) do
    if index > 1 then io.write(",") end
    io.write(receipt)
end
io.write("]")
