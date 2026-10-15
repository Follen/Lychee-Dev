local ADDON_NAME, ns = ...

-- One shared API surface for verified client differences needed by the
-- workbench. Every value that crosses a trust boundary is checked for secrecy
-- before it is compared, formatted, indexed or branched on; failures open to a
-- harmless visible state instead of throwing.
ns.Compat = {
    CanReload = function()
        if type(IsLoggedIn) ~= "function" or type(InCombatLockdown) ~= "function" then return nil,"input_observation_unavailable" end
        local loggedIn,combat=IsLoggedIn(),InCombatLockdown()
        if issecretvalue and (issecretvalue(loggedIn) or issecretvalue(combat)) then return nil,"input_observation_unavailable" end
        if loggedIn~=true then return nil,"input_not_logged_in" end
        if combat~=false then return nil,"input_combat_lockdown" end
        return true
    end,
    ObserveChatFocus = function(owner, callback)
        if not EventRegistry or type(EventRegistry.RegisterCallback) ~= "function" then return end
        EventRegistry:RegisterCallback("ChatFrame.OnEditBoxFocusGained", callback, owner)
        EventRegistry:RegisterCallback("ChatFrame.OnEditBoxFocusLost", callback, owner)
    end,
    StopObservingChatFocus = function(owner)
        if not EventRegistry or type(EventRegistry.UnregisterCallback) ~= "function" then return end
        EventRegistry:UnregisterCallback("ChatFrame.OnEditBoxFocusGained", owner)
        EventRegistry:UnregisterCallback("ChatFrame.OnEditBoxFocusLost", owner)
    end,
    ReloadUI = function()
        if C_UI and type(C_UI.Reload) == "function" then C_UI.Reload(); return true end
        if type(ReloadUI) == "function" then ReloadUI(); return true end
        return nil, "reload_unavailable"
    end,
    -- Pinned PlayerScriptDocumentation declares this on Retail/Classic/Titan.
    InWorld = function()
        if type(IsPlayerInWorld)~="function" then return nil end
        local ok,value=pcall(IsPlayerInWorld)
        if not ok or (issecretvalue and issecretvalue(value)) or type(value)~="boolean" then return nil end
        return value
    end,
    LoadInputSlot = function(name)
        if type(name)~="string" then return nil,"slot_name_invalid" end
        local index=tonumber(name:match("^Lychee Dev Slot (%d+)$"))
        if not index or index<1 or index>ns.SlotProtocol.Count or name~=string.format("Lychee Dev Slot %02d",index) then return nil,"slot_name_invalid" end
        if type(C_AddOns)~="table" or type(C_AddOns.LoadAddOn)~="function" then return nil,"slot_loader_unavailable" end
        local ok,loaded,reason=pcall(C_AddOns.LoadAddOn,name)
        if not ok or (issecretvalue and (issecretvalue(loaded) or issecretvalue(reason))) then return nil,"slot_load_failed" end
        if loaded~=true and reason=="DISABLED" then return nil,"slot_addon_disabled" end
        return loaded==true and true or nil,type(reason)=="string" and reason or "slot_load_failed"
    end,
    -- Caller has verified this managed slot's metadata and current actor.
    -- These APIs change enable settings only; no slot is loaded or consumed.
    EnsureInputSlotEnabled = function(name,character)
        if type(C_AddOns)~="table" or type(C_AddOns.GetAddOnEnableState)~="function" then return nil,"slot_enable_state_unavailable" end
        local function state()
            local ok,value=pcall(C_AddOns.GetAddOnEnableState,name,character)
            if not ok or (issecretvalue and issecretvalue(value)) or type(value)~="number"
                or (value~=0 and value~=1 and value~=2) then return nil end
            return value
        end
        local before=state()
        if before==nil then return nil,"slot_enable_state_failed" end
        if before~=0 then return false end
        if type(C_AddOns.EnableAddOn)~="function" or type(C_AddOns.SaveAddOns)~="function" then return nil,"slot_enable_unavailable" end
        if not pcall(C_AddOns.EnableAddOn,name,character) then return nil,"slot_enable_failed" end
        local after=state()
        if after==nil or after==0 then return nil,"slot_enable_failed" end
        return true
    end,
    SaveInputSlots = function()
        if type(C_AddOns)~="table" or type(C_AddOns.SaveAddOns)~="function" then return nil,"slot_enable_save_unavailable" end
        if not pcall(C_AddOns.SaveAddOns) then return nil,"slot_enable_save_failed" end
        return true
    end,
    ReceiverChord = function(key)
        if (issecretvalue and issecretvalue(key)) or type(key)~="string" then return nil end
        local number=string.match(key,"^F([1-9]%d?)$")
        if key~="[" and key~="]" and not (number and tonumber(number)<=12) then return nil end
        local function down(callback)
            if type(callback)~="function" then return false end
            local ok,value=pcall(callback)
            return ok and not (issecretvalue and issecretvalue(value)) and value==true
        end
        if not down(IsControlKeyDown) or not down(IsAltKeyDown) then return nil end
        return "ALT-CTRL-"..(down(IsShiftKeyDown) and "SHIFT-" or "")..key
    end,
    MonotonicSeconds = function()
        if type(GetTime) ~= "function" then return nil end
        local ok, value = pcall(GetTime)
        if not ok or (issecretvalue and issecretvalue(value)) or type(value) ~= "number"
            or value ~= value or value < 0 or value == math.huge then return nil end
        return value
    end,
    GetAddOnMetadata = function(addonName, field)
        if type(C_AddOns) ~= "table" or type(C_AddOns.GetAddOnMetadata) ~= "function" then
            return nil
        end
        if (issecretvalue and (issecretvalue(addonName) or issecretvalue(field))) then
            return nil
        end
        local succeeded, value = pcall(C_AddOns.GetAddOnMetadata, addonName, field)
        if not succeeded or (issecretvalue and issecretvalue(value)) then
            return nil
        end
        return value
    end,

    GetMouseFocus = function()
        if type(GetMouseFoci) ~= "function" then
            return nil
        end
        local succeeded, foci = pcall(GetMouseFoci)
        if not succeeded or (issecretvalue and issecretvalue(foci))
            or type(foci) ~= "table"
            or (issecretvalue and issecretvalue(foci[1])) or not foci[1] then
            return nil
        end
        return foci[1]
    end,
}
