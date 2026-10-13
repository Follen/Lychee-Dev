-- Isolated prototype. Lua 5.1 SHA-256/HMAC; no addon/product entrypoint.
-- Arithmetic stays exact below 2^53; all word additions reduce modulo 2^32.
local floor, char, byte, sub, rep, concat = math.floor, string.char, string.byte, string.sub, string.rep, table.concat
local MOD = 4294967296
local native = rawget(_G, 'bit')
local nativeXor, nativeAnd, nativeRor
if type(native) == 'table' and type(native.bxor) == 'function'
    and type(native.band) == 'function' and type(native.ror) == 'function' then
    nativeXor, nativeAnd, nativeRor = native.bxor, native.band, native.ror
end
local xor4, and4 = {}, {}
if not nativeXor then
for a=0,15 do
    xor4[a], and4[a] = {}, {}
    for b=0,15 do
        local x,y,p,aa,bb=0,0,1,a,b
        for _=1,4 do
            local ax,bx=aa%2,bb%2
            if ax~=bx then x=x+p end
            if ax==1 and bx==1 then y=y+p end
            aa,bb,p=floor(aa/2),floor(bb/2),p*2
        end
        xor4[a][b],and4[a][b]=x,y
    end
end
end
local function bits(a,b,lookup)
    local v,p=0,1
    for _=1,8 do
        v=v+lookup[a%16][b%16]*p
        a,b,p=floor(a/16),floor(b/16),p*16
    end
    return v
end
local function portableXor(a,b,c)
    local v=bits(a,b,xor4)
    if c then return bits(v,c,xor4) end
    return v
end
local function portableAnd(a,b) return bits(a,b,and4) end
local function portableRor(a,n) return floor(a/2^n)+(a%2^n)*2^(32-n) end
-- Native signed words have the same low 32 bits. XOR/AND/rotate feed only
-- another word operation or a sum reduced modulo 2^32; byte XOR stays 0..255.
-- Capture the operations once instead of branching/normalizing every round.
local xor = nativeXor or portableXor
local band = nativeAnd or portableAnd
local ror = nativeRor or portableRor
local function word(a) return char(floor(a/16777216)%256,floor(a/65536)%256,floor(a/256)%256,a%256) end
local K={
0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2}
local function sha(raw)
    assert(type(raw)=="string" and #raw<=524416,"sha_budget")
    local length=#raw
    local data=raw..char(128)..rep(char(0),(55-length)%64)..word(0)..word(length*8)
    local h={0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19}
    for offset=1,#data,64 do
        local w={}
        for i=0,15 do
            local a,b,c,d=byte(data,offset+i*4,offset+i*4+3)
            w[i]=a*16777216+b*65536+c*256+d
        end
        for i=16,63 do
            local a,b=w[i-15],w[i-2]
            w[i]=(w[i-16]+xor(ror(a,7),ror(a,18),floor(a/8))+w[i-7]+xor(ror(b,17),ror(b,19),floor(b/1024)))%MOD
        end
        local a,b,c,d,e,f,g,hh=unpack(h)
        for i=0,63 do
            local ch=xor(band(e,f),band(MOD-1-e,g))
            local maj=xor(band(a,b),band(a,c),band(b,c))
            local t1=(hh+xor(ror(e,6),ror(e,11),ror(e,25))+ch+K[i+1]+w[i])%MOD
            local t2=(xor(ror(a,2),ror(a,13),ror(a,22))+maj)%MOD
            hh,g,f,e,d,c,b,a=g,f,e,(d+t1)%MOD,c,b,a,(t1+t2)%MOD
        end
        local v={a,b,c,d,e,f,g,hh}
        for i=1,8 do h[i]=(h[i]+v[i])%MOD end
    end
    local out={}
    for i=1,8 do out[i]=word(h[i]) end
    return concat(out)
end
local function hex(v) return (v:gsub('.',function(c) return string.format('%02x',byte(c)) end)) end
local function hmac(key,value)
    assert(type(key)=="string" and #key<=1024 and type(value)=="string" and #value<=8192,"hmac_budget")
    if #key>64 then key=sha(key) end
    key=key..rep(char(0),64-#key)
    local inner,outer={},{}
    for i=1,64 do inner[i]=char(xor(byte(key,i),0x36)); outer[i]=char(xor(byte(key,i),0x5c)) end
    return hex(sha(concat(outer)..sha(concat(inner)..value)))
end
return {sha256=function(v) return hex(sha(v)) end,hmac=hmac}
