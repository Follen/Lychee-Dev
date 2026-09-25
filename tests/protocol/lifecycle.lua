local root = assert(arg[1])
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
local profiles = {
    { toc = "Mainline", version = "12.1.0", interface = 120100, product = "retail" },
    { toc = "Mists", version = "5.5.4", interface = 50504, product = "classic" },
    { toc = "Wrath", version = "3.80.2", interface = 38002, product = "titan" },
    { toc = "Forever", version = "1.60.1", interface = 16001, product = "forever" },
}

for _, profile in ipairs(profiles) do
    for _, scenario in ipairs({ "fresh", "existing", "enabled", "command_conflict", "future", "invalid", "mismatch", "secret" }) do
        local frames = {}
        SlashCmdList, SLASH_LYCHEETOOLKIT1 = {}, nil
        local foreignHandler = function() end
        if scenario == "command_conflict" then SlashCmdList.LYCHEETOOLKIT = foreignHandler end
        CreateFrame = function(kind)
            assert(kind == "Frame")
            local frame = { events = {}, scripts = {} }
            function frame:RegisterEvent(event) self.events[event] = true end
            function frame:UnregisterAllEvents() self.events = {} end
            function frame:SetScript(event, callback) self.scripts[event] = callback end
            frames[#frames + 1] = frame
            return frame
        end
        GetBuildInfo = function()
            local interface = scenario == "mismatch" and 99999 or profile.interface
            return scenario == "secret" and secret or profile.version, "12345", "date", interface
        end
        local actor = { character = "Paladin", realm = "Realm", guid = "Player-1-12345" }
        UnitFullName = function(unit) assert(unit == "player"); return actor.character, actor.realm end
        UnitGUID = function(unit) assert(unit == "player"); return actor.guid end
        LycheeDevDB, DumperDB = { retained = true }, { retained = true }
        local old, older = LycheeDevDB, DumperDB
        LycheeToolkitDB = nil
        if scenario == "existing" then LycheeToolkitDB = { schema = 1, custom = "keep", options = { bridgeEnabled = false } } end
        if scenario == "enabled" then LycheeToolkitDB = { schema = 1, options = { bridgeEnabled = true } } end
        if scenario == "future" then LycheeToolkitDB = { schema = 2, future = true } end
        if scenario == "invalid" then LycheeToolkitDB = { schema = 1, options = "bad" } end
        local before = LycheeToolkitDB
        local ns = {}
        local toc = assert(io.open(root .. "/Lychee Dev.toc", "r"))
        for line in toc:lines() do
            line = line:gsub("\r", "")
            if line ~= "" and line:sub(1, 1) ~= "#" then
                assert(loadfile(root .. "/" .. line:gsub("\\", "/")))("Lychee Dev", ns)
            end
        end
        toc:close()
        assert(LycheeToolkitDB == before, "database read/write before ADDON_LOADED")
        assert(not ns.Startup.ready and ns.Persistence.Current() == nil)
        assert((scenario == "command_conflict" or next(SlashCmdList) == nil) and SLASH_LYCHEETOOLKIT1 == nil)
        assert(ns.Controls.Handle("bridge on") == nil and ns.Controls.Register() == nil)
        assert(#frames == 1 and frames[1].events.ADDON_LOADED)
        local callback = frames[1].scripts.OnEvent
        callback(frames[1], "ADDON_LOADED", "OtherAddon")
        assert(LycheeToolkitDB == before and not ns.Startup.ready)
        callback(frames[1], "ADDON_LOADED", "Lychee Dev")
        assert(next(frames[1].events) == nil and frames[1].scripts.OnEvent == nil)
        assert(#frames == 1 and LycheeDevDB == old and DumperDB == older)
        if scenario == "command_conflict" then
            assert(ns.Startup.ready and ns.Startup.commandFailure == "command_registration_conflict")
            assert(SlashCmdList.LYCHEETOOLKIT == foreignHandler and SLASH_LYCHEETOOLKIT1 == nil)
        elseif scenario == "fresh" or scenario == "existing" or scenario == "enabled" then
            assert(ns.Startup.ready and ns.Startup.identity.product == profile.product)
            local state = assert(ns.Persistence.Current())
            assert(state.schema == 1 and type(state.reports) == "table")
            assert((state.options.bridgeEnabled == true) == (scenario == "enabled"))
            assert(SLASH_LYCHEETOOLKIT1 == "/dev" and type(SlashCmdList.LYCHEETOOLKIT) == "function")
            assert(SLASH_LYCHEETOOLKIT2 == nil, "unexpected command alias")
            local status = assert(ns.Controls.Handle("status"))
            assert(status.enabled == (scenario == "enabled") and not status.inputReady)
            local heldReports = state.reports
            heldReports.retained = { body = "keep" }
            status = assert(ns.Controls.Handle("  BRIDGE ON  "))
            assert(status.enabled and not status.inputReady and status.reason == "transport_unavailable")
            local nonce = string.rep("a", 32)
            assert(ns.Session.Bind(secret) == nil and ns.Session.Bind("short") == nil)
            status = assert(ns.Controls.Handle("bridge bind " .. nonce))
            assert(status.bound and status.session.sessionNonce == nonce and not status.inputReady)
            status.session.character = "Tampered copy"
            assert(ns.Session.Current().character == "Paladin")
            local issued = assert(ns.Session.NextIdentity())
            assert(issued.sequence == 1)
            issued.sequence = 9007199254740991
            assert(ns.Session.NextIdentity().sequence == 2, "caller changed sequence counter")
            assert(ns.Session.Bind(nonce))
            local rejected, reason = ns.Session.Bind(string.rep("b", 32))
            assert(rejected == nil and reason == "session_busy")
            actor.guid = "Player-1-99999"
            rejected, reason = ns.Session.Current()
            assert(rejected == nil and reason == "session_actor_changed")
            assert(ns.Session.Bind(nonce))
            actor.character = secret
            assert(ns.Session.Current() == nil)
            actor.character = "Paladin"
            assert(ns.Session.Current() == nil, "invalidated session revived")
            assert(ns.Session.Bind(nonce))
            assert(ns.Controls.Handle("bridge unbind"))
            assert(ns.Session.Current() == nil)
            actor.realm = nil
            local unavailable, unavailableReason = ns.Session.Bind(nonce)
            assert(unavailable == nil and unavailableReason == "actor_unavailable")
            actor.realm = "Realm"
            assert(ns.Session.Bind(nonce))
            assert(state.options.bridgeEnabled == true and state.reports == heldReports)
            assert(ns.Controls.Handle("bridge maybe") == nil and state.options.bridgeEnabled == true)
            assert(ns.Controls.Handle(secret) == nil and ns.Controls.Handle(string.rep("x", 129)) == nil)
            status = assert(ns.Controls.Handle("bridge off"))
            assert(not status.enabled and state.options.bridgeEnabled == nil and state.reports.retained.body == "keep")
            assert(ns.Session.Current() == nil and ns.Session.Bind(nonce) == nil)
            local handler = SlashCmdList.LYCHEETOOLKIT
            assert(ns.Controls.Register() and handler == SlashCmdList.LYCHEETOOLKIT)
            local originalPrint, replies = print, {}
            print = function(text) replies[#replies + 1] = text end
            handler("bridge on")
            assert(state.options.bridgeEnabled == true)
            handler("bridge off")
            handler("unknown")
            print = originalPrint
            assert(#replies == 3 and string.find(replies[1], '"inputReady":false', 1, true))
            assert(string.find(replies[3], "usage: /dev", 1, true))
            assert(state.options.bridgeEnabled == nil and state.reports.retained.body == "keep")
            assert(#frames == 1 and next(frames[1].events) == nil)
            if before then assert(state == before) end
            if scenario == "existing" then assert(state.custom == "keep") end
            assert(ns.Controls.Handle("bridge on"))
            assert(ns.Session.Bind(nonce))
            local replacement = { schema = 1, options = { bridgeEnabled = true }, reports = {} }
            LycheeToolkitDB = replacement
            assert(ns.Persistence.Current() == replacement, "stale profile root")
            local oldBinding, rootReason = ns.Session.Current()
            assert(oldBinding == nil and rootReason == "session_state_changed")
            assert(ns.Persistence.Load() and ns.Persistence.Current() == replacement)
        else
            assert(not ns.Startup.ready and ns.Startup.reason)
            assert(LycheeToolkitDB == before, "unsupported state was overwritten")
            assert(ns.Persistence.Current() == nil)
        end
    end
end
print("lifecycle: four clients passed")
