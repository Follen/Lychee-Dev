local ADDON_NAME, ns = ...

-- Identity markers answer "who is in this window" for one host trigger. They
-- bind no session, write no SavedVariables, and grant no input authority. The
-- module creates nothing until Trigger is called.
local pending
local focusEvent = "ChatFrame.OnEditBoxFocusLost"

local function restricted(value)
    return issecretvalue and issecretvalue(value)
end

local function validNonce(nonce)
    return not restricted(nonce) and type(nonce) == "string"
        and #nonce == 32 and string.match(nonce, "^[0-9a-f]+$") ~= nil
end

local function cancelWait()
    local watch = pending
    pending = nil
    if watch then watch.registry:UnregisterCallback(focusEvent, watch) end
end

-- A request-scoped operation in flight owns this window's display. Scanning
-- must never clobber another agent's receipt, so uncertainty fails closed.
local function idle()
    if type(ns.ProbeQueue) == "table" and type(ns.ProbeQueue.Busy) == "function" then
        local busy, reason = ns.ProbeQueue.Busy()
        if busy == nil then return nil, reason end
        if busy then return nil, "identity_busy" end
    end
    if type(ns.Reentry) == "table" and type(ns.Reentry.Busy) == "function" then
        local busy, reason = ns.Reentry.Busy()
        if busy == nil then return nil, reason end
        if busy then return nil, "identity_busy" end
    end
    return true
end

local function observe(probeNonce)
    local build, buildFailure = ns.Platform.ObserveBuild()
    if not build then return nil, buildFailure end
    local actor, actorFailure = ns.Platform.ObserveActor()
    local actorState = "ok"
    if not actor then
        if actorFailure == "actor_restricted_identity" then
            actorState = "actor_restricted"
        else
            actorState = "no_actor"
        end
    end
    local inputReady, inputReason = true, nil
    if type(ns.Platform.ObserveInputState) == "function" then
        local eligible, reason = ns.Platform.ObserveInputState()
        if eligible == true then
            inputReady = true
        else
            inputReady = false
            if type(reason) == "string" and #reason > 0 and not restricted(reason) then
                inputReason = reason
            else
                inputReason = "input_observation_unavailable"
            end
        end
    else
        inputReady = false
        inputReason = "input_observation_unavailable"
    end
    local marker = {
        schema = "lycheedev.signal.v1", release = ns.Release, kind = "identity",
        probeNonce = probeNonce, actorState = actorState,
        sessionNonce = "", requestId = "",
        product = build.product, build = build.build,
        sequence = 0, inputReady = inputReady,
    }
    if inputReason then marker.inputReason = inputReason end
    if actorState == "ok" then
        marker.character = actor.character
        marker.realm = actor.realm
        marker.guid = actor.guid
    end
    return ns.CaptureWriter.Encode(marker, 4096)
end

ns.Identity = {
    CancelWait = cancelWait,
    -- Returns the encoded identity receipt; the caller displays it. Echoing the
    -- host probe nonce correlates exactly one trigger with one receipt.
    Trigger = function(nonce)
        if not validNonce(nonce) then return nil, "identity_invalid_nonce" end
        local ready, reason = idle()
        if not ready then return nil, reason end
        cancelWait()
        return observe(nonce)
    end,
    -- A fresh observation for the same trigger, used to replace the first
    -- display once keyboard focus has been released.
    Refresh = function(nonce)
        if not validNonce(nonce) then return nil, "identity_invalid_nonce" end
        local ready, reason = idle()
        if not ready then return nil, reason end
        return observe(nonce)
    end,
    -- One request-scoped callback, never a timer or permanent focus listener.
    -- It only prompts a fresh observation; it proves nothing by itself.
    WhenInputReady = function(callback)
        if type(callback) ~= "function" then return nil, "input_invalid_callback" end
        if pending then return nil, "input_wait_busy" end
        if type(ns.Platform.ObserveInputState) ~= "function" then return nil, "input_observation_unavailable" end
        local eligible, reason = ns.Platform.ObserveInputState()
        if eligible then callback(true); return true end
        if reason ~= "input_keyboard_focus" then return nil, reason or "input_observation_unavailable" end
        if type(EventRegistry) ~= "table" or type(EventRegistry.RegisterCallback) ~= "function"
            or type(EventRegistry.UnregisterCallback) ~= "function" then
            return nil, "input_events_unavailable"
        end
        local watch = { registry = EventRegistry }
        pending = watch
        watch.registry:RegisterCallback(focusEvent, function()
            if pending ~= watch then return end
            cancelWait()
            local now, observeFailure = ns.Platform.ObserveInputState()
            callback(now == true, observeFailure)
        end, watch)
        return true
    end,
}
