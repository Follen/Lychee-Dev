local _, ns = ...

-- Immutable binary strings are retained by the transport. Finding one is not
-- publication proof: the host must request a fresh confirmation of live state.
local function u32(n)
    return string.char(n%256,math.floor(n/256)%256,math.floor(n/65536)%256,math.floor(n/16777216)%256)
end
local function token(value)
    return not (issecretvalue and issecretvalue(value)) and type(value)=="string"
        and #value==32 and value:match("^[0-9a-f]+$")~=nil
end
local function raw(hex)
    return (hex:gsub("..",function(pair)return string.char(tonumber(pair,16))end))
end
local ZERO=string.rep("0",32)
local ZERO_RAW=string.rep("\0",16)
-- One current runtime, never a token history. Call only after token validation:
-- a cache hit must not bypass the current secret/type/length checks.
local lastRuntime,lastRuntimeRaw
local function runtimeRaw(runtime)
    if runtime~=lastRuntime then lastRuntime,lastRuntimeRaw=runtime,raw(runtime) end
    return lastRuntimeRaw
end
local function checksum(value)
    return tonumber(ns.CaptureWriter.DigestBytes(value),16)
end
ns.MemoryProtocol = {
    Token=token, Checksum=checksum,
    Encode=function(nonce,runtime,ticket,kind,state,sequence,payload)
        if not token(nonce) or not token(runtime) or not token(ticket)
            or (issecretvalue and (issecretvalue(payload) or issecretvalue(kind) or issecretvalue(state) or issecretvalue(sequence)))
            or type(payload)~="string" or #payload>524288
            or type(kind)~="number" or kind%1~=0 or kind<1 or kind>5
            or type(state)~="number" or state%1~=0 or state<1 or state>255
            or type(sequence)~="number" or sequence<0 or sequence>=4294967296 or sequence%1~=0 then
            return nil,"memory_record_invalid"
        end
        local runtimeBytes=runtimeRaw(runtime)
        local nonceBytes=nonce==runtime and runtimeBytes or nonce==ZERO and ZERO_RAW or raw(nonce)
        local ticketBytes=ticket==ZERO and ZERO_RAW or ticket==runtime and runtimeBytes or raw(ticket)
        local header="LYCMEM05"..nonceBytes..runtimeBytes..ticketBytes
            ..string.char(kind,state,0,0)..u32(sequence)..u32(#payload)..u32(checksum(payload))..u32(80)
        return header..u32(checksum(header))..payload.."LYCEND05"..nonceBytes..runtimeBytes
    end,
}
