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
local function checksum(value)
    return tonumber(ns.CaptureWriter.DigestBytes(value),16)
end
ns.MemoryProtocol = {
    Token=token, Checksum=checksum,
    Encode=function(nonce,runtime,ticket,kind,state,sequence,payload)
        if not token(nonce) or not token(runtime) or not token(ticket)
            or type(payload)~="string" or #payload>524288
            or kind<1 or kind>5 or state<1 or state>255
            or type(sequence)~="number" or sequence<0 or sequence>=4294967296 or sequence%1~=0 then
            return nil,"memory_record_invalid"
        end
        local header="LYCMEM06"..raw(nonce)..raw(runtime)..raw(ticket)
            ..string.char(kind,state,0,0)..u32(sequence)..u32(#payload)..u32(checksum(payload))..u32(80)
        return header..u32(checksum(header))..payload.."LYCEND06"..raw(nonce)..raw(runtime)
    end,
}
