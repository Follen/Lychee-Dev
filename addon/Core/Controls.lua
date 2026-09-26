local ADDON_NAME, ns = ...

local registered = false
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end

ns.Controls = {
    Handle = function(message)
        if restricted(message) or type(message) ~= "string" or #message > 256 then
            return nil, "command_invalid_input"
        end
        local trimmed = string.match(message, "^%s*(.-)%s*$")
        local command = string.lower(trimmed)
        local nonce = string.match(command, "^bridge bind ([0-9a-f]+)$")
        local identify = string.match(command, "^bridge identify ([0-9a-f]+)$")
        local prefix, verb, requestId = string.match(trimmed, "^(%S+)%s+(%S+)%s+([%w_%-]+)$")
        local action, reportSequence
        local verifyPrefix, verifyVerb, verifyRequest, cleanupNonce = string.match(trimmed, "^(%S+)%s+(%S+)%s+([%w_%-]+)%s+([0-9a-f]+)$")
        if verifyPrefix and string.lower(verifyPrefix) == "bridge" and string.lower(verifyVerb) == "verify"
            and #verifyRequest <= 128 and #cleanupNonce == 32 then
            action, requestId = "verify", verifyRequest
        end
        if verifyPrefix and string.lower(verifyPrefix) == "bridge" and string.lower(verifyVerb) == "clean"
            and #verifyRequest <= 128 and #cleanupNonce == 32 then
            action, requestId = "clean", verifyRequest
        end
        if verifyPrefix and string.lower(verifyPrefix) == "bridge" and string.lower(verifyVerb) == "prepare"
            and #verifyRequest <= 128 and #cleanupNonce == 32 then
            action, requestId = "prepare", verifyRequest
        end
        if verifyPrefix and string.lower(verifyPrefix) == "bridge" and string.lower(verifyVerb) == "ack"
            and #verifyRequest <= 128 and #cleanupNonce <= 16 and string.match(cleanupNonce, "^[1-9]%d*$") then
            action, requestId, reportSequence = "ack", verifyRequest, tonumber(cleanupNonce)
        end
        if verifyPrefix and string.lower(verifyPrefix) == "bridge" and string.lower(verifyVerb) == "bugs"
            and #verifyRequest <= 128 and #cleanupNonce <= 3 and string.match(cleanupNonce,"^%d+$") then
            action,requestId,reportSequence="bugs",verifyRequest,tonumber(cleanupNonce)
        end
        if verifyPrefix and string.lower(verifyPrefix) == "bridge" and string.lower(verifyVerb) == "bugs-ack"
            and #verifyRequest <= 128 and #cleanupNonce <= 16 and string.match(cleanupNonce,"^[1-9]%d*$") then
            action,requestId,reportSequence="bugs-ack",verifyRequest,tonumber(cleanupNonce)
        end
        if verifyPrefix and string.lower(verifyPrefix) == "bridge" and string.lower(verifyVerb) == "flush"
            and #verifyRequest <= 128 and #cleanupNonce == 32 then
            action,requestId="flush",verifyRequest
        end
        if command == "bridge ready" then action = "ready" end
        if command == "bridge hide" then action = "hide" end
        local resetNonce = string.match(command, "^bridge reset ([0-9a-f]+)$")
        if resetNonce and #resetNonce == 32 then action = "reset" end
        if prefix and string.lower(prefix) == "bridge" and #requestId <= 128 then
            verb = string.lower(verb)
            if verb == "load" or verb == "run" or verb == "reload" then action = verb end
            if verb == "refresh" and #requestId == 32 and string.match(requestId,"^[0-9a-f]+$") then action = verb end
        end
        if command ~= "" and command ~= "status" and command ~= "connect" and command ~= "disconnect" and command ~= "bridge on" and command ~= "bridge off"
            and command ~= "bridge unbind" and not nonce and not identify and not action then
            return nil, "usage: /dev status | connect | disconnect"
        end
        if not ns.Startup.ready then return nil, ns.Startup.reason or "addon_not_ready" end
        -- Bare /dev toggles the workbench. Everything below keeps the exact
        -- status/connect/disconnect/bridge parsing contract.
        if command == "" then
            local shown, reason = ns.Workbench.Toggle()
            if reason then
                print("|cffd83b4eLychee Dev:|r " .. tostring(reason))
            end
            return true
        end
        local state, failure = ns.Persistence.Current()
        if not state then return nil, failure end
        if command == "connect" then
            if not state.options then state.options = {} end
            local previous = state.options.bridgeEnabled
            state.options.bridgeEnabled = true
            local connected, reason = ns.Session.Connect()
            if not connected then state.options.bridgeEnabled = previous; return nil, reason end
            action = "ready"
        end
        if identify then
            -- Identity markers are session-free and never disturb another
            -- operation's displayed receipt: Trigger fails closed when busy.
            local receipt, reason = ns.Identity.Trigger(identify)
            if not receipt then return nil, reason end
            local function refreshIdentity() return ns.Identity.Refresh(identify) end
            local shown, displayFailure = ns.ReceiptView.ShowIdentity(receipt, refreshIdentity)
            if not shown then return nil, displayFailure end
            ns.Identity.WhenInputReady(function(ready)
                if not ready then return end
                local refreshed, refreshFailure = ns.Identity.Refresh(identify)
                if not refreshed then print("Lychee Dev: " .. refreshFailure); return end
                local visible, showFailure = ns.ReceiptView.ShowIdentity(refreshed, refreshIdentity)
                if not visible then ns.ReceiptView.Hide(); print("Lychee Dev: " .. showFailure) end
            end)
            return receipt
        end
        if action == "hide" then
            -- Dismissal answers no new receipt: success must not re-display a
            -- card or print, otherwise the command would defeat itself.
            local dismissed, reason = ns.ReceiptView.Dismiss()
            if not dismissed then return nil, reason end
            return true
        end
        if action == "reset" then
            -- Recovery trigger, not an operation: the nonce-correlated receipt
            -- proves this exact trigger reached a live runtime and unblocked
            -- its queue. The card displays session-free, like an identity.
            local receipt, reason = ns.ProbeQueue.Reset(resetNonce)
            if not receipt then return nil, reason end
            local shown, displayFailure = ns.ReceiptView.ShowIdentity(receipt, nil)
            if not shown then return nil, displayFailure end
            return receipt
        end
        if action then
            if action == "refresh" then return ns.Reentry.Refresh(requestId) end
            if action == "prepare" then return ns.Reentry.LoadQueue(requestId, cleanupNonce) end
            if action == "clean" then return ns.Reentry.Reload(requestId, cleanupNonce) end
            if action == "reload" then return ns.Reentry.Reload(requestId) end
            if action == "flush" then return ns.Reentry.Flush(requestId,cleanupNonce) end
            -- Remove the old optical signal before loading or executing work.
            -- Preserve request ID case; only command words are case-insensitive.
            ns.Session.CancelInputWait()
            ns.ReceiptView.Hide()
            local receipt, reason
            if action == "load" then receipt, reason = ns.ProbeQueue.Load(requestId)
            elseif action == "ready" then receipt, reason = ns.Session.ReadyReceipt()
            elseif action == "ack" then receipt, reason = ns.ProbeQueue.Acknowledge(requestId, reportSequence)
            elseif action == "bugs" then receipt, reason = ns.FaultRunner.Run(requestId,reportSequence)
            elseif action == "bugs-ack" then receipt, reason = ns.FaultRunner.Acknowledge(requestId,reportSequence)
            elseif action == "verify" then receipt, reason = ns.ProbeQueue.VerifyRetired(requestId, cleanupNonce)
            else receipt, reason = ns.ProbeRunner.Dispatch(requestId) end
            if not receipt then return nil, reason end
            local paired = action == "run" or action == "ack" or action == "bugs" or action == "bugs-ack"
            local refresh
            if paired or action == "ready" or action == "load" then
                refresh = function()
                    if action == "load" then return ns.ProbeRunner.RefreshLoaded(requestId) end
                    local current, failure, signal = ns.Session.ReadyReceipt()
                    if not current then return nil, failure end
                    if paired then return receipt, current, signal end
                    return current
                end
            end
            local shown, displayFailure = ns.ReceiptView.Show(receipt, nil, refresh)
            if not shown then return nil, displayFailure end
            if action == "load" or action == "ready" or action == "run" or action == "ack" or action == "bugs" or action == "bugs-ack" then
                ns.Session.WhenInputReady(function(ready)
                    if not ready then return end
                    local refreshed, refreshFailure, signal
                    if action == "ready" or action == "run" or action == "ack" or action == "bugs" or action == "bugs-ack" then refreshed, refreshFailure, signal = ns.Session.ReadyReceipt()
                    else refreshed, refreshFailure = ns.ProbeRunner.RefreshLoaded(requestId) end
                    if not refreshed then print("Lychee Dev: " .. refreshFailure); return end
                    local visible, reason
                    if paired then visible, reason = ns.ReceiptView.Show(receipt, refreshed, refresh, signal)
                    else visible, reason = ns.ReceiptView.Show(refreshed, nil, refresh) end
                    if not visible then ns.ReceiptView.Hide(); print("Lychee Dev: " .. reason); return end
                    if action ~= "run" and action ~= "ack" and action ~= "bugs" and action ~= "bugs-ack" then receipt = refreshed end
                end)
            end
            if command == "connect" then
                local active = ns.Session.Current()
                return { connected = true, character = active.character, realm = active.realm,
                    product = ns.Startup.identity.product, build = ns.Startup.identity.build }
            end
            return receipt
        end
        if command == "bridge on" or command == "bridge off" or command == "disconnect" then
            if not state.options then state.options = {} end
            -- Disabled is the default, not a persisted copy of that default.
            -- Reports survive opt-out; disabling never acknowledges or deletes.
            state.options.bridgeEnabled = command == "bridge on" and true or nil
            if command ~= "bridge on" then
                if ns.Reentry then ns.Reentry.Cancel(true) end
                ns.Session.Release(); ns.ReceiptView.Hide()
            end
        end
        if command == "bridge unbind" then
            if ns.Reentry then ns.Reentry.Cancel(true) end
            ns.Session.Release(); ns.ReceiptView.Hide()
        end
        if nonce then
            local bound, reason = ns.Session.Bind(nonce)
            if not bound then return nil, reason end
        end
        local enabled = state.options and state.options.bridgeEnabled == true or false
        local session = ns.Session.Current()
        return {
            release = ns.Release, product = ns.Startup.identity.product,
            build = ns.Startup.identity.build, enabled = enabled,
            bound = session ~= nil, session = session,
            -- Configuration is not input eligibility. Session binding and the
            -- live transport must supply their own observed readiness later.
            inputReady = false, reason = enabled and "transport_unavailable" or "bridge_disabled",
        }
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
