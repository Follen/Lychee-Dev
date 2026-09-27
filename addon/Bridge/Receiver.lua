local _, ns = ...

local frame, editor, deadline
local active, stage, challenge, nonce, epoch, actor, phase
local sequence = 0
local accepted, acceptedCount = {}, 0
local boot, epochRoot
local counter = 0
local constructionFailed
local function restricted(value) return issecretvalue and issecretvalue(value) end
local function randomHex()
    return string.format("%04x%04x%04x%04x", math.random(0, 65535),
        math.random(0, 65535), math.random(0, 65535), math.random(0, 65535))
end
local function encodeReceipt(state, reason, receiptStage)
    if restricted(reason) then reason = "receiver_dispatch_failed" end
    sequence = sequence + 1
    local focus = editor and editor:HasFocus()
    if restricted(focus) then focus = false end
    local signal = { schema = "lycheedev.signal.v1", kind = "receiver_" .. state,
        release = ns.Release, product = ns.Startup.identity.product,
        build = ns.Startup.identity.build, receiverNonce = nonce,
        runtimeEpoch = epoch, sequence = sequence,
        inputReady = active and focus == true or false,
        accepted = state == "accepted" }
    if state == "ready" then
        signal.receiverProtocol = "intent-v2"
        signal.reportScope = "character-v1"
        local bindings = ns.ReceiverBindings.Current()
        if not bindings then return nil, "receiver_bindings_unavailable" end
        signal.wakeBinding, signal.submitBinding, signal.closeBinding =
            bindings.wake, bindings.submit, bindings.close
        local session = ns.Session.Current()
        if session then
            signal.sessionNonce = session.sessionNonce
            signal.priorSessionSequence = session.sequence
            signal.priorSessionEpoch = session.runtimeEpoch
        end
    end
    if actor then
        signal.character, signal.realm, signal.guid = actor.character, actor.realm, actor.guid
    end
    local input = receiptStage or stage
    if input then
        signal.requestId, signal.attemptId = input.requestId, input.attemptId
        signal.bodyBytes, signal.bodyAdler32 = input.bodyBytes, input.bodyAdler32
    end
    if challenge and state == "commit_ready" then signal.commitNonce = challenge end
    if reason then signal.errorCode = reason end
    local encoded, failure = ns.CaptureWriter.EncodeSignal(signal, 2048)
    if not encoded then return nil, failure end
    return encoded
end
local function receipt(state, reason, receiptStage)
    local encoded, failure = encodeReceipt(state, reason, receiptStage)
    if not encoded then return nil, failure end
    local shown, failure = ns.ReceiptView.ShowIdentity(encoded)
    if shown and ns.ActivityView then ns.ActivityView.Anchor() end
    return shown, failure
end
local function release(state, reason)
    active = false
    phase = state
    if deadline then deadline:Cancel(); deadline = nil end
    if frame then frame:SetScript("OnUpdate", nil) end
    if editor then editor:ClearFocus() end
    if frame then frame:Hide(); frame:UnregisterAllEvents() end
    if ns.ActivityView then ns.ActivityView.Receiving(false) end
    if state then receipt(state, reason) end
end
local function reject(reason)
    -- A malformed frame cannot define a trusted request identity. Any staged
    -- identity shown here came from an earlier, fully validated frame.
    release("rejected", reason or "receiver_invalid_frame")
end
local function command(staged)
    local action, requestId, arg = staged.action, staged.requestId, staged.argument
    if action == "identify" then return "bridge identify " .. arg end
    if action == "connect" then return "connect" end
    if action == "reset" then return "bridge reset " .. arg end
    if action == "ready" or action == "hide" then return "bridge " .. action end
    if action == "refresh" then return "bridge refresh " .. arg end
    if action == "ack" or action == "bugs" or action == "bugs-ack" or
        action == "verify" or action == "prepare" or action == "clean" or action == "flush" or action == "observe" or action == "finish" then
        return "bridge " .. action .. " " .. requestId .. " " .. arg
    end
    return "bridge " .. action .. " " .. requestId
end
local function accept()
    if not active or phase ~= "committed" or not stage or not challenge then return end
    local staged = stage
    -- These three actions create or execute request-scoped work. The other
    -- actions are read-only/idempotent or own an atomic reentry ticket in their
    -- business module, so they do not consume receiver replay slots.
    local track = staged.action == "load" or staged.action == "run" or staged.action == "bugs"
    local businessKey = staged.action .. ":" .. staged.requestId
    local body = ns.InputProtocol.Body(staged.action, staged.requestId, staged.argument)
    local previous = track and accepted[businessKey] or nil
    if previous and previous.body ~= body then reject("receiver_request_changed"); return end
    if previous then
        release("accepted", previous.errorCode)
        phase, stage, challenge = nil, nil, nil
        return
    end
    if track and acceptedCount >= 256 then reject("receiver_accept_limit"); return end
    -- Record before business dispatch, including an exception or reload. The
    -- same request is never invoked again in this runtime.
    if track then
        accepted[businessKey] = { body = body }
        acceptedCount = acceptedCount + 1
    end
    release("accepted")
    -- Delivery ends before business dispatch. The replay fence above survives
    -- independently of focus, this input buffer and the currently visible QR.
    -- No host CLOSE or transient accepted card is needed to accept a new input.
    phase, stage, challenge = nil, nil, nil
    local dispatchFailure
    local ok, result, failure = pcall(ns.Controls.Handle, command(staged))
    if not ok or restricted(result) then dispatchFailure = "receiver_dispatch_exception"
    elseif not result then dispatchFailure = not restricted(failure) and
        type(failure) == "string" and #failure <= 64 and
        string.match(failure, "^[A-Za-z0-9_%-]+$") and failure or "receiver_dispatch_failed" end
    if dispatchFailure then receipt("accepted", dispatchFailure, staged) end
    if track then accepted[businessKey].errorCode = dispatchFailure end
    -- An exact ACK retires this operation's receiver entries only after the
    -- business dispatcher succeeds. Unresolved work keeps its replay fence.
    if ok and not restricted(result) and result and
        (staged.action == "ack" or staged.action == "bugs-ack") then
        for _, verb in ipairs({"load", "run", "bugs", "ack", "bugs-ack"}) do
            local key = verb .. ":" .. staged.requestId
            if accepted[key] then accepted[key] = nil; acceptedCount = acceptedCount - 1 end
        end
    end
end
local function submit()
    if not active or phase ~= "staged" or not stage then return end
    -- A key carries no attempt identity. It only issues a fresh optical
    -- challenge; a separate correlated LDC1 frame is needed to dispatch.
    challenge = randomHex()
    phase = "challenge"
    editor:SetText("")
    return receipt("commit_ready")
end
local function onTextChanged(self)
    if not active then return end
    local text = self:GetText()
    if restricted(text) or type(text) ~= "string" then reject("receiver_restricted_input"); return end
    if phase == "committed" then
        if text ~= stage.commitFrame then reject("receiver_commit_changed") end
        return
    end
    -- The initial wake key may land after focus. Strip that single punctuation
    -- immediately; waiting for key-up stalled the proven background sender.
    if (phase == "ready" and ns.ReceiverBindings.MatchKey(text, "wake"))
        or (phase == "challenge" and ns.ReceiverBindings.MatchKey(text, "submit")) then
        self:SetText(""); return
    end
    if phase == "staged" then
        if text ~= stage.frame then self:SetText(stage.frame) end
        return
    end
    if #text > ns.InputProtocol.MaxBytes then reject("receiver_capacity"); return end
    if text == "" then return end
    if not string.match(text, "^[A-Za-z0-9_:%-]+$") then reject("receiver_encoding"); return end
    local expected = phase == "challenge" and "LDC1:" or "LDB1:"
    if #text <= #expected and string.sub(expected, 1, #text) ~= text then reject("receiver_format"); return end
    -- Only a complete trailing checksum permits a parse. Partial frames stay
    -- inert until they finish or hit the independent hard deadline.
    local colonCount = select(2, string.gsub(text, ":", ""))
    local requiredColons = phase == "challenge" and 5 or 8
    if colonCount ~= requiredColons or
        not string.match(text, ":[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]$") then return end
    if phase == "challenge" then
        local matched, failure = ns.InputProtocol.Commit(text, stage, challenge)
        if not matched then reject(failure); return end
        -- Text changes carry no hardware-event context. Keep the validated
        -- commit inert until the host's final Enter; reload and other protected
        -- operations must be dispatched from that key event.
        stage.commitFrame, phase = text, "committed"
    elseif phase == "ready" then
        local parsed, failure = ns.InputProtocol.Stage(text, nonce, epoch)
        if not parsed then reject(failure); return end
        parsed.frame = text
        stage, phase = parsed, "staged"
        receipt("staged")
    end
end
local function makeFrame()
    frame = CreateFrame("Frame", nil, UIParent)
    -- A new WoW Frame starts visible. Keep the full-screen input shield hidden
    -- while every child control and script is being constructed.
    frame:Hide()
    frame:SetAllPoints(UIParent); frame:SetFrameStrata("FULLSCREEN_DIALOG")
    frame:EnableMouse(true); frame:EnableMouseWheel(true)
    frame:SetScript("OnMouseDown", function() if active then editor:SetFocus() end end)
    frame:SetScript("OnMouseWheel", function() end)
    editor = CreateFrame("EditBox", nil, frame)
    editor:SetSize(1, 1)
    editor:SetPoint("TOPLEFT", frame, "TOPLEFT", 0, 0)
    editor:SetFont(ns.Theme.font, 14, "")
    -- Alpha hides both native text and caret without removing keyboard focus.
    -- This control must remain shown until release(), including staged input.
    editor:SetAlpha(0)
    editor:SetAutoFocus(false); editor:SetMaxLetters(ns.InputProtocol.MaxBytes + 1)
    editor:SetScript("OnTextChanged", onTextChanged)
    editor:SetScript("OnKeyDown", function(_, key)
        if not active or restricted(key) then return end
        -- Focused punctuation is receiver-local. The isolated Retail lab proved
        -- delivery of these keys, not physical modifier-state distinction.
        if ns.ReceiverBindings.MatchKey(key, "close") then release("cancelled", "receiver_manual_close")
        elseif ns.ReceiverBindings.MatchKey(key, "submit") then submit() end
    end)
    editor:SetScript("OnEnterPressed", accept)
    editor:SetScript("OnEscapePressed", function() end)
    editor:SetScript("OnTabPressed", function() end)
    -- The shield owns focus until acceptance, explicit close, combat or the
    -- deadline. A click/focus notification cannot cancel a valid transaction.
    editor:SetScript("OnEditFocusLost", function(self) if active then self:SetFocus() end end)
    frame:SetScript("OnEvent", function(_, event)
        if active then release("cancelled", event == "PLAYER_REGEN_DISABLED"
            and "receiver_combat" or "receiver_world_changed") end
    end)
end
local function rollbackWake(partialConstruction)
    active = false
    phase, stage, challenge = nil, nil, nil
    if deadline then pcall(deadline.Cancel, deadline); deadline = nil end
    if frame then
        pcall(frame.SetScript, frame, "OnUpdate", nil)
        pcall(frame.Hide, frame)
        pcall(frame.UnregisterAllEvents, frame)
    end
    if editor then pcall(editor.ClearFocus, editor) end
    if ns.ActivityView then pcall(ns.ActivityView.Receiving, false) end
    if partialConstruction then
        -- A partial frame cannot be reused. Fence repeated wake attempts in
        -- this runtime so a persistent API failure cannot allocate more UI.
        constructionFailed = true
        editor = nil
    end
end
local function canOpen()
    if constructionFailed then return nil, "receiver_ui_unavailable" end
    if not ns.ReceiverBindings or not ns.ReceiverBindings.Current() then return nil, "receiver_bindings_unavailable" end
    if not ns.Startup.ready then return nil, "addon_not_ready" end
    local state, failure = ns.Persistence.Bridge()
    if not state then return nil, failure end
    local ok, inCombat = pcall(InCombatLockdown)
    if not ok or restricted(inCombat) or inCombat ~= false then return nil, "receiver_combat" end
    local actorValue, actorFailure = ns.Platform.ObserveActor()
    if not actorValue then return nil, actorFailure end
    local session = ns.Session.Current()
    if session then return state, actorValue, session.runtimeEpoch end
    if state ~= epochRoot then
        local previous = state.runtimeEpoch or 0
        if restricted(previous) or type(previous) ~= "number" or previous < 0 or
            previous % 1 ~= 0 or previous >= 9007199254740991 then return nil, "receiver_invalid_epoch" end
        state.runtimeEpoch = previous + 1
        epochRoot = state
    end
    return state, actorValue, state.runtimeEpoch
end
local function wake()
    if active then return true end -- No deadline extension and no text reset.
    local state, observed, currentEpoch = canOpen()
    if not state then return nil, observed end
    if not frame then
        if not pcall(makeFrame) then
            rollbackWake(true)
            return nil, "receiver_ui_unavailable"
        end
    end
    -- Prepare the deadline and all visible copy before exposing the input
    -- shield. Any API exception leaves no focus, event, timer or visible frame.
    local ok, shown, failure = pcall(function()
        if not boot then boot = string.format("%04x%04x", math.random(0, 65535), math.random(0, 65535)) end
        counter = counter + 1
        if counter > 4294967295 then return nil, "receiver_nonce_exhausted" end
        nonce, epoch, actor, stage, challenge = boot .. string.format("%08x", counter) .. randomHex(), currentEpoch, observed, nil, nil
        phase = "ready"
        editor:SetText("")
        if type(C_Timer) ~= "table" or type(C_Timer.NewTimer) ~= "function" then
            return nil, "receiver_timer_unavailable"
        end
        deadline = C_Timer.NewTimer(20, function() if active then release("timeout", "receiver_deadline") end end)
        if not deadline then return nil, "receiver_timer_unavailable" end
        active = true
        if ns.ActivityView then ns.ActivityView.Receiving(true) end
        frame:RegisterEvent("PLAYER_REGEN_DISABLED")
        frame:RegisterEvent("PLAYER_LEAVING_WORLD")
        frame:Show()
        editor:SetFocus()
        return receipt("ready")
    end)
    if not ok or not shown then
        rollbackWake(false)
        return nil, ok and (failure or "receiver_ui_unavailable") or "receiver_ui_unavailable"
    end
    if ns.StartupBeacon then ns.StartupBeacon.Stop() end
    return shown, failure
end
local function close()
    -- Emergency cancellation belongs only to the input surface. It cannot
    -- acknowledge business work, hide its receipt or gate the next command.
    if active then
        release("cancelled", "receiver_manual_close")
    else
        if frame then
            pcall(frame.Hide, frame)
            pcall(frame.SetScript, frame, "OnUpdate", nil)
            pcall(frame.UnregisterAllEvents, frame)
        end
        if editor then pcall(editor.ClearFocus, editor) end
    end
    phase, stage, challenge = nil, nil, nil
    return true
end

ns.Receiver = {
    Wake = wake, Submit = submit, Close = close,
    IsActive = function() return active == true end,
    Register = function()
        if not ns.ReceiverBindings then return nil, "receiver_bindings_unavailable" end
        return ns.ReceiverBindings.Register({ wake = wake, submit = submit, close = close })
    end,
}
