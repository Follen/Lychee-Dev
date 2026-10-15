local _,ns=...
local function u32(n)return string.char(n%256,math.floor(n/256)%256,math.floor(n/65536)%256,math.floor(n/16777216)%256)end
local function token(v)return not(issecretvalue and issecretvalue(v)) and type(v)=="string" and #v==32 and v:match("^[0-9a-f]+$")~=nil end
local function raw(v)return(v:gsub("..",function(p)return string.char(tonumber(p,16))end))end
local function checksum(v)return tonumber(ns.CaptureWriter.DigestBytes(v),16)end
ns.NativeWire={Token=token,Checksum=checksum,Encode=function(nonce,runtime,ticket,kind,state,seq,payload)
    if not token(nonce) or not token(runtime) or not token(ticket) or type(payload)~="string" or #payload>524288
        or kind<1 or kind>5 or state<1 or state>255 or seq<0 or seq>=4294967296 then return nil,"native_wire_invalid"end
    local h="LYCMEM07"..raw(nonce)..raw(runtime)..raw(ticket)..string.char(kind,state,0,0)..u32(seq)..u32(#payload)..u32(checksum(payload))..u32(80)
    return h..u32(checksum(h))..payload.."LYCEND07"..raw(nonce)..raw(runtime)
end}
