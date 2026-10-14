local ADDON_NAME, ns = ...
local registered = false
local function restricted(value) return issecretvalue and issecretvalue(value) end
ns.Controls = {
    Handle = function(message)
        if restricted(message) or type(message) ~= "string" or #message > 256 then return nil,"command_invalid_input" end
        local command=string.lower(string.match(message,"^%s*(.-)%s*$"))
        if command~="" and command~="status" and command~="connect" and command~="disconnect"
            and command~="bridge on" and command~="bridge off" and command~="receiver reset" then
            if command:match("^bridge ") then return nil,"command_retired: use the memory-slot CLI" end
            return nil,"usage: /dev status | connect | disconnect | receiver reset"
        end
        if not ns.Startup.ready then return nil,ns.Startup.reason or "addon_not_ready" end
        if command=="" then
            local shown,reason=ns.Workbench.Toggle()
            if reason then print("|cffd83b4eLychee Dev:|r "..tostring(reason)) end
            return true
        end
        local state,failure=ns.Persistence.Current()
        if not state then return nil,failure end
        if command=="receiver reset" then
            local profile,reason=ns.ReceiverBindings.Reset()
            if not profile then return nil,reason end
            ns.InputState.Refresh()
            return "receiver bindings reset"
        elseif command=="connect" or command=="bridge on" then
            state.options.bridgeEnabled=true
            return ns.SlotRuntime.Start()
        elseif command=="disconnect" or command=="bridge off" then
            state.options.bridgeEnabled=false
            ns.InputState.Stop()
            ns.SlotRuntime.Close()
            ns.ActivityView.Stop()
            return true
        end
        return {transport="Lycheedev mailbox protocol v1",enabled=ns.Persistence.BridgeEnabled()}
    end,
    Register = function()
        if registered then return true end
        if not ns.Startup.ready then return nil, "addon_not_ready" end
        if restricted(SlashCmdList) or type(SlashCmdList) ~= "table"
            or restricted(SlashCmdList.LYCHEETOOLKIT) or SlashCmdList.LYCHEETOOLKIT ~= nil
            or restricted(SLASH_LYCHEETOOLKIT1) or SLASH_LYCHEETOOLKIT1 ~= nil then
            return nil, "command_registration_conflict"
        end
        SLASH_LYCHEETOOLKIT1 = "/dev"
        SlashCmdList.LYCHEETOOLKIT = function(message)
            local result, failure = ns.Controls.Handle(message)
            if not result then print("Lychee Dev: " .. failure); return end
            if result == true then return end
            if type(result) == "string" then print("Lychee Dev: " .. result); return end
            local text, reason = ns.CaptureWriter.Encode(result, 4096)
            print("Lychee Dev: " .. (text or reason))
        end
        registered = true
        return true
    end,
}
