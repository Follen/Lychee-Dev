local ADDON_NAME, ns = ...

local binding
local boundRoot
local generation = 0
local runtimeEpoch, epochRoot
local pendingInput
local focusEvent = "ChatFrame.OnEditBoxFocusLost"
local function cancelInput()
    local pending = pendingInput
    pendingInput = nil
    if pending then pending.registry:UnregisterCallback(focusEvent, pending) end
end
local function context()
    local state, failure = ns.Persistence.Current()
    if not state then return nil, failure end
    if not ns.Startup.ready or not state.options or state.options.bridgeEnabled ~= true then
        return nil, "bridge_disabled"
    end
    local actor, reason = ns.Platform.ObserveActor()
    return actor, reason, state
end
local function copy(value)
    return { sessionNonce = value.sessionNonce, character = value.character,
        realm = value.realm, guid = value.guid, sequence = value.sequence, generation = value.generation,
        runtimeEpoch = value.runtimeEpoch }
end
local function current()
    local actor, failure, state = context()
    if not actor then cancelInput(); binding, boundRoot = nil, nil; return nil, failure end
    if not binding then return nil, "session_unbound" end
    -- Retain only root identity for invalidation, never read settings through it.
    if state ~= boundRoot then cancelInput(); binding, boundRoot = nil, nil; return nil, "session_state_changed" end
    if (issecretvalue and issecretvalue(state.runtimeEpoch)) or state.runtimeEpoch ~= runtimeEpoch then
        cancelInput(); binding, boundRoot = nil, nil; return nil, "session_epoch_changed"
    end
    if actor.character ~= binding.character or actor.realm ~= binding.realm or actor.guid ~= binding.guid then
        cancelInput()
        binding, boundRoot = nil, nil
        return nil, "session_actor_changed"
    end
    return copy(binding)
end

ns.Session = {
    Current = current,
    CancelInputWait = cancelInput,
    Connect = function()
        local active = current()
        if active then return active end
        local actor, failure = context()
        if not actor then return nil, failure end
        if generation >= 9007199254740991 then return nil, "session_generation_exhausted" end
        -- A visible correlation marker, not a password or input permission.
        -- A counter prevents reuse in this runtime without reseeding WoW's RNG;
        -- native process identity and actor checks distinguish other runtimes.
        local nextGeneration = generation + 1
        local nonce = string.format("%04x%04x%04x%04x%08x%08x",
            math.random(0, 65535), math.random(0, 65535),
            math.random(0, 65535), math.random(0, 65535),
            math.floor(nextGeneration / 4294967296), nextGeneration % 4294967296)
        return ns.Session.Bind(nonce)
    end,
    ReadyReceipt = function()
        local active, failure = current()
        if not active then return nil, failure end
        local eligible = false
        if type(ns.Platform.ObserveInputState) == "function" then
            eligible = ns.Platform.ObserveInputState() == true
        end
        local identity, identityFailure = ns.Session.NextIdentity()
        if not identity then return nil, identityFailure end
        return ns.CaptureWriter.EncodeSignal({
            schema = "lycheedev.signal.v1", release = ns.Release, kind = "ready",
            sessionNonce = identity.sessionNonce, requestId = "",
            character = identity.character, realm = identity.realm, guid = identity.guid,
            product = ns.Startup.identity.product, build = ns.Startup.identity.build,
            sequence = identity.sequence, inputReady = eligible,
            runtimeEpoch = identity.runtimeEpoch,
        }, 4096)
    end,
    -- One request-scoped callback, never a timer or permanent focus listener.
    -- A focus event only prompts a fresh observation; it is not itself proof.
    WhenInputReady = function(callback)
        if type(callback) ~= "function" then return nil, "input_invalid_callback" end
        local active, failure = current()
        if not active then return nil, failure end
        if pendingInput then return nil, "input_wait_busy" end
        if type(ns.Platform.ObserveInputState) ~= "function" then return nil, "input_observation_unavailable" end
        local ready, reason = ns.Platform.ObserveInputState()
        if ready then callback(true); return true end
        if reason ~= "input_keyboard_focus" then return nil, reason end
        if type(EventRegistry) ~= "table" or type(EventRegistry.RegisterCallback) ~= "function"
            or type(EventRegistry.UnregisterCallback) ~= "function" then
            return nil, "input_events_unavailable"
        end
        local pending = { registry = EventRegistry, generation = active.generation }
        pendingInput = pending
        pending.registry:RegisterCallback(focusEvent, function()
            if pendingInput ~= pending then return end
            cancelInput()
            local now, sessionFailure = current()
            if not now or now.generation ~= pending.generation then
                callback(false, sessionFailure or "input_session_changed")
                return
            end
            local eligible, observeFailure = ns.Platform.ObserveInputState()
            callback(eligible == true, observeFailure)
        end, pending)
        return true
    end,
    NextIdentity = function(afterSequence)
        local active, failure = current()
        if not active then return nil, failure end
        if (issecretvalue and issecretvalue(afterSequence)) then return nil, "session_invalid_sequence" end
        if afterSequence == nil then afterSequence = 0 end
        if type(afterSequence) ~= "number" or afterSequence < 0 or afterSequence > 9007199254740991
            or afterSequence % 1 ~= 0 then return nil, "session_invalid_sequence" end
        local previous = math.max(binding.sequence, afterSequence)
        if previous >= 9007199254740991 then return nil, "session_sequence_exhausted" end
        binding.sequence = previous + 1
        return copy(binding)
    end,
    Bind = function(nonce)
        if (issecretvalue and issecretvalue(nonce)) or type(nonce) ~= "string"
            or #nonce ~= 32 or not string.match(nonce, "^[0-9a-f]+$") then
            return nil, "session_invalid_nonce"
        end
        local actor, failure, state = context()
        if not actor then cancelInput(); binding, boundRoot = nil, nil; return nil, failure end
        local previousEpoch = state.runtimeEpoch
        if previousEpoch == nil then previousEpoch = 0 end
        if (issecretvalue and issecretvalue(previousEpoch)) or type(previousEpoch) ~= "number"
            or previousEpoch < 0 or previousEpoch % 1 ~= 0 or previousEpoch >= 9007199254740991 then
            return nil, "session_invalid_epoch"
        end
        -- Allocate once per Lua runtime, only after opt-in and actor validation.
        -- Persist a monotonic marker, never the binding nonce or input permission.
        -- A profile re-root must not silently reuse another root's epoch.
        if state == epochRoot and previousEpoch ~= runtimeEpoch then return nil, "session_epoch_changed" end
        if binding and state == boundRoot and actor.character == binding.character and actor.realm == binding.realm and actor.guid == binding.guid then
            if binding.sessionNonce ~= nonce then return nil, "session_busy" end
            return copy(binding)
        end
        if generation >= 9007199254740991 then return nil, "session_generation_exhausted" end
        if state ~= epochRoot then
            local last = math.max(previousEpoch, runtimeEpoch or 0)
            if last >= 9007199254740991 then return nil, "session_invalid_epoch" end
            runtimeEpoch = last + 1
            state.runtimeEpoch = runtimeEpoch
            epochRoot = state
        end
        cancelInput()
        generation = generation + 1
        binding = { sessionNonce = nonce, character = actor.character, realm = actor.realm, guid = actor.guid, sequence = 0, generation = generation, runtimeEpoch = runtimeEpoch }
        boundRoot = state
        return copy(binding)
    end,
    Release = function()
        cancelInput()
        binding, boundRoot = nil, nil
    end,
}
