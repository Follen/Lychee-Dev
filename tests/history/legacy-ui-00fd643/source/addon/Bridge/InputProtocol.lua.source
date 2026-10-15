local _, ns = ...

-- Both frames are ASCII and anchored. The checksum covers every byte before
-- the final colon; it detects accidental input pollution, not a secret actor.
local function restricted(value) return issecretvalue and issecretvalue(value) end
local function hex(value, length)
    return type(value) == "string" and #value == length and
        string.match(value, "^[0-9a-f]+$") ~= nil
end
local function request(value)
    return type(value) == "string" and #value >= 1 and #value <= 80 and
        string.match(value, "^[%w_%-]+$") ~= nil
end
local function actionArgument(action, requestId, argument)
    if action == "identify" or action == "reset" then
        return requestId == "-" and hex(argument, 32)
    elseif action == "connect" or action == "ready" or action == "hide" then
        return requestId == "-" and argument == "-"
    elseif action == "refresh" then
        return requestId == "-" and hex(argument, 32)
    elseif action == "load" or action == "run" or action == "reload" then
        return requestId ~= "-" and argument == "-"
    elseif action == "verify" or action == "prepare" or action == "clean" or action == "flush" or action == "observe" or action == "finish" then
        return requestId ~= "-" and hex(argument, 32)
    elseif action == "ack" or action == "bugs-ack" then
        return requestId ~= "-" and #argument <= 16 and string.match(argument, "^[1-9]%d*$") ~= nil
    elseif action == "bugs" then
        return requestId ~= "-" and #argument <= 3 and string.match(argument, "^%d+$") ~= nil
    end
    return false
end
local function checkedPrefix(text, marker)
    if restricted(text) or type(text) ~= "string" or #text > 256 then return nil, "receiver_capacity" end
    if not string.match(text, "^[A-Za-z0-9_:%-]+$") then return nil, "receiver_encoding" end
    local prefix, checksum = string.match(text, "^(.*):([0-9a-f]+)$")
    if not prefix or #checksum ~= 8 or string.sub(prefix, 1, #marker) ~= marker then
        return nil, "receiver_format"
    end
    if ns.CaptureWriter.DigestBytes(prefix) ~= checksum then return nil, "receiver_checksum" end
    return prefix
end

ns.InputProtocol = {
    MaxBytes = 256,
    Body = function(action, requestId, argument)
        return action .. "|" .. requestId .. "|" .. argument
    end,
    Stage = function(text, expectedNonce, expectedEpoch)
        local prefix, failure = checkedPrefix(text, "LDB1:")
        if not prefix then return nil, failure end
        local nonce, epoch, requestId, attempt, action, argument, length = string.match(prefix,
            "^LDB1:([0-9a-f]+):([0-9]+):([%w_%-]+):([0-9a-f]+):([a-z%-]+):([%w_%-]+):([0-9]+)$")
        if not nonce or not hex(nonce, 32) or not hex(attempt, 16) or not request(requestId)
            or not actionArgument(action, requestId, argument) then return nil, "receiver_format" end
        if expectedEpoch < 1 or expectedEpoch >= 9007199254740991 or
            nonce ~= expectedNonce or tonumber(epoch) ~= expectedEpoch or
            tostring(expectedEpoch) ~= epoch then return nil, "receiver_identity" end
        local body = ns.InputProtocol.Body(action, requestId, argument)
        if tonumber(length) ~= #body or tostring(#body) ~= length then return nil, "receiver_length" end
        return { receiverNonce = nonce, runtimeEpoch = expectedEpoch, requestId = requestId,
            attemptId = attempt, action = action, argument = argument,
            bodyBytes = #body, bodyAdler32 = ns.CaptureWriter.DigestBytes(body) }
    end,
    Commit = function(text, staged, challenge)
        local prefix, failure = checkedPrefix(text, "LDC1:")
        if not prefix then return nil, failure end
        local nonce, attempt, receivedChallenge, digest = string.match(prefix,
            "^LDC1:([0-9a-f]+):([0-9a-f]+):([0-9a-f]+):([0-9a-f]+)$")
        if not nonce or not hex(nonce, 32) or not hex(attempt, 16) or
            not hex(receivedChallenge, 16) or not hex(digest, 8) then return nil, "receiver_format" end
        if nonce ~= staged.receiverNonce or attempt ~= staged.attemptId or
            digest ~= staged.bodyAdler32 or receivedChallenge ~= challenge then
            return nil, "receiver_commit_mismatch" end
        return true
    end,
}
