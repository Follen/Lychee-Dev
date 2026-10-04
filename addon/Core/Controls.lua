local ADDON_NAME, ns = ...
local registered = false
local function restricted(value) return issecretvalue and issecretvalue(value) end
ns.Controls = {
    Handle = function(message)
        if restricted(message) or type(message) ~= "string" or #message > 256 then return nil, "command_invalid_input" end
        local command = string.lower(string.match(message, "^%s*(.-)%s*$"))
        if not ns.Startup.ready then return nil, ns.Startup.reason or "addon_not_ready" end
        if command == "" then return ns.Workbench.Toggle() end
        if command == "status" then return ns.DuplexRuntime.Snapshot() end
        local state, failure = ns.Persistence.Current()
        if not state then return nil, failure end
        if command == "connect" or command == "bridge on" then
            local previous = state.options.bridgeEnabled
            state.options.bridgeEnabled = true
            local ok, reason = ns.DuplexRuntime.Enable()
            if not ok then state.options.bridgeEnabled = previous; return nil, reason end
            return true
        end
        if command == "disconnect" or command == "bridge off" then
            state.options.bridgeEnabled = false
            return ns.DuplexRuntime.Disable()
        end
        return nil, "usage: /dev | status | connect | disconnect"
    end,
    Register = function()
        if registered then return true end
        if not ns.Startup.ready then return nil, "addon_not_ready" end
        if restricted(SlashCmdList) or type(SlashCmdList) ~= "table"
            or restricted(SlashCmdList.LYCHEETOOLKIT) or SlashCmdList.LYCHEETOOLKIT ~= nil
            or restricted(SLASH_LYCHEETOOLKIT1) or SLASH_LYCHEETOOLKIT1 ~= nil then return nil, "command_registration_conflict" end
        SLASH_LYCHEETOOLKIT1 = "/dev"
        SlashCmdList.LYCHEETOOLKIT = function(message)
            local result, failure = ns.Controls.Handle(message)
            if not result then print("Lychee Dev: " .. tostring(failure)); return end
            if result == true then return end
            if type(result) == "string" then print("Lychee Dev: " .. result); return end
            local text, reason = ns.CaptureWriter.Encode(result, 4096)
            print("Lychee Dev: " .. (text or reason))
        end
        registered = true
        return true
    end,
}
