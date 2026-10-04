local _, ns = ...

-- Small SHA-256 implementation for Lua 5.1. WoW's bit library is used when
-- present; the nibble fallback keeps protocol fixtures and compatible clients
-- independent of a particular bit library. All arithmetic is unsigned mod 2^32.
local MOD, HALF = 4294967296, 2147483648
local bitlib = rawget(_G, "bit") or rawget(_G, "bit32")
local xorNibble, andNibble = {}, {}
for a = 0, 15 do
    for b = 0, 15 do
        local x, y, xo, an, place = a, b, 0, 0, 1
        for _ = 1, 4 do
            local xb, yb = x % 2, y % 2
            if xb ~= yb then xo = xo + place end
            if xb == 1 and yb == 1 then an = an + place end
            x, y, place = math.floor(x / 2), math.floor(y / 2), place * 2
        end
        xorNibble[a * 16 + b], andNibble[a * 16 + b] = xo, an
    end
end
local function u32(v) return v % MOD end
local function purePair(a, b, lookup)
    a, b = u32(a), u32(b)
    local out, place = 0, 1
    for _ = 1, 8 do
        local ai, bi = a % 16, b % 16
        out = out + lookup[ai * 16 + bi] * place
        a, b, place = math.floor(a / 16), math.floor(b / 16), place * 16
    end
    return out
end
local function bxor2(a, b)
    if bitlib then return u32(bitlib.bxor(a, b)) end
    return purePair(a, b, xorNibble)
end
local function band2(a, b)
    if bitlib then return u32(bitlib.band(a, b)) end
    return purePair(a, b, andNibble)
end
local function bxor3(a, b, c) return bxor2(bxor2(a, b), c) end
local function band3(a, b, c) return band2(band2(a, b), c) end
local function bnot(a)
    if bitlib then return u32(bitlib.bnot(a)) end
    return 4294967295 - u32(a)
end
local function rshift(a, n)
    if bitlib then
        local f = bitlib.rshift or bitlib.brshift
        if f then return u32(f(a, n)) end
    end
    return math.floor(u32(a) / 2 ^ n)
end
local function lshift(a, n)
    if bitlib then
        local f = bitlib.lshift or bitlib.blshift
        if f then return u32(f(a, n)) end
    end
    return u32(u32(a) * 2 ^ n)
end
local function ror(a, n) return u32(rshift(a, n) + lshift(a, 32 - n)) end
local function add(...) local n = 0; for i = 1, select("#", ...) do n = n + select(i, ...) end; return n % MOD end
local K = {
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
}
local INITIAL = {0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19}
local function word(s, i)
    local a,b,c,d=s:byte(i,i+3)
    return ((a*256+b)*256+c)*256+d
end
local function put32(n)
    n=u32(n)
    return string.char(math.floor(n/16777216)%256,math.floor(n/65536)%256,math.floor(n/256)%256,n%256)
end
local function block(h, w, s, offset)
    -- Every slot is overwritten; reuse one private schedule per digest context.
    for i=0,15 do w[i]=word(s,offset+i*4) end
    for i=16,63 do
        local x,y=w[i-15],w[i-2]
        local s0=bxor3(ror(x,7),ror(x,18),rshift(x,3))
        local s1=bxor3(ror(y,17),ror(y,19),rshift(y,10))
        w[i]=add(w[i-16],s0,w[i-7],s1)
    end
    local a,b,c,d,e,f,g,hh=h[1],h[2],h[3],h[4],h[5],h[6],h[7],h[8]
    for i=0,63 do
        local s1=bxor3(ror(e,6),ror(e,11),ror(e,25))
        local ch=bxor2(band2(e,f),band2(bnot(e),g))
        local t1=add(hh,s1,ch,K[i+1],w[i])
        local s0=bxor3(ror(a,2),ror(a,13),ror(a,22))
        local maj=bxor3(band2(a,b),band2(a,c),band2(b,c))
        local t2=add(s0,maj)
        hh,g,f,e,d,c,b,a=g,f,e,add(d,t1),c,b,a,add(t1,t2)
    end
    h[1],h[2],h[3],h[4]=add(h[1],a),add(h[2],b),add(h[3],c),add(h[4],d)
    h[5],h[6],h[7],h[8]=add(h[5],e),add(h[6],f),add(h[7],g),add(h[8],hh)
end
local API={}
function API.New()
    local h={};for i=1,8 do h[i]=INITIAL[i] end
    return {h=h,work={},buffer="",bytes=0,done=false}
end
function API.Update(ctx, text)
    if type(ctx)~="table" or ctx.done or type(text)~="string" then return nil,"sha256_state_invalid" end
    if ctx.bytes + #text >= HALF then return nil,"sha256_input_limit" end
    ctx.bytes=ctx.bytes+#text
    local data=ctx.buffer..text
    local last=#data-#data%64
    for offset=1,last,64 do block(ctx.h,ctx.work,data,offset) end
    ctx.buffer=data:sub(last+1)
    return true
end
function API.Final(ctx)
    if type(ctx)~="table" or ctx.done then return nil,"sha256_state_invalid" end
    local bitlo=(ctx.bytes*8)%MOD
    local bithi=math.floor(ctx.bytes/536870912)%MOD
    local pad="\128"..string.rep("\0",(55-ctx.bytes)%64)..put32(bithi)..put32(bitlo)
    local data=ctx.buffer..pad
    for offset=1,#data,64 do block(ctx.h,ctx.work,data,offset) end
    local raw={};for i=1,8 do raw[i]=put32(ctx.h[i]) end
    ctx.done=true;ctx.buffer="";ctx.h=nil;ctx.work=nil
    return table.concat(raw)
end
function API.Hex(raw)
    if type(raw)~="string" then return nil end
    return (raw:gsub(".",function(c)return string.format("%02x",c:byte())end))
end
function API.Digest(text)
    local ctx=API.New();local ok,err=API.Update(ctx,text);if not ok then return nil,err end
    local raw;raw,err=API.Final(ctx);if not raw then return nil,err end
    return raw,API.Hex(raw)
end
API.NativeBit=bitlib~=nil
ns.SHA256=API
