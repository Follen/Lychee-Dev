-- Stock Lua records freeze ownership; direct assignment models host WPM.
local root=assert(arg[1])
local frozen=setmetatable({}, {__mode="k"})
table.freeze=function(row)frozen[row]=true;return row end
table.isfrozen=function(row)return frozen[row]==true end
local ns={CaptureWriter={}}
assert(loadfile(root.."/addon/Bridge/SHA256.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/addon/Bridge/CaptureWriter.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/addon/Bridge/DuplexProtocol.lua"))("Lychee Dev",ns)
local P,S=ns.DuplexProtocol,ns.SHA256
local Z16,Z32=string.rep("\0",16),string.rep("\0",32)
local function raw(hex)return (hex:gsub("..",function(pair)return string.char(tonumber(pair,16))end))end
local function u32(n)return string.char(n%256,math.floor(n/256)%256,math.floor(n/65536)%256,math.floor(n/16777216)%256)end
local function u64(n)return u32(n%4294967296)..u32(math.floor(n/4294967296))end
local identity={runtime="00112233445566778899aabbccddeeff",arenaGeneration="10112233445566778899aabbccddeeff",
    actorBindingId="40112233445566778899aabbccddeeff",actorGUID="Player-1-1",character="Paladin",realm="Realm",build="12.1.0.12345",product="retail",release="3.1.1"}
local function header(kind,source,opts)
    opts=opts or {};source=source or ""
    local seq=opts.seq or 1;local rid=opts.id or string.format("%032x",seq)
    local challenge=opts.challenge or string.rep("0",32)
    local budget=opts.budget or 120000;local utc=7654321
    local request=opts.digest and raw(opts.digest) or P.RequestDigest(raw(rid),raw(identity.actorBindingId),budget,0,utc,#source,source)
    local first="LYCMBX01"..u32(kind)..raw(identity.runtime)..raw(opts.arena or identity.arenaGeneration)
        ..raw(opts.session or "20112233445566778899aabbccddeeff")..raw(opts.owner or "30112233445566778899aabbccddeeff")
        ..raw(identity.actorBindingId)..raw(rid)..raw(opts.mid or string.format("%032x",10000+seq))..raw(challenge)
        ..u64(opts.fence or 11)..u64(opts.pub or seq)..u64(seq)..u64(opts.attempt or 1)..u64(utc)..u32(budget)..u32(#source)
        ..u32(opts.total or #source)..u32(kind==P.Kind.frame and 1 or 0)..u32(kind==P.Kind.frame and 1 or 0)..request
    assert(#first==232)
    local checksum=kind==P.Kind.frame and (opts.ack and S.Digest("LYCMBX/result-ack/v1\0"..opts.ack) or Z32) or S.Digest("LYCMBX/frame/v1\0"..first..source)
    local normalized=first..checksum..Z32..Z16..string.rep("\0",8)
    local wire=first..checksum..S.Digest("LYCMBX/header/v1\0"..normalized)..u64((opts.pub or seq)*2)..u64((opts.pub or seq)*2)..string.rep("\0",8)..source
    return wire
end
local used=setmetatable({}, {__mode="k"})
local function setRow(row,bytes)
    local count=math.ceil(#bytes/4)
    for n=1,math.max(count,used[row] or 0) do row[n]=0 end
    for at=1,#bytes,4 do local a,b,c,d=bytes:byte(at,at+3);row[(at-1)/4+1]=(a or 0)+(b or 0)*256+(c or 0)*65536+(d or 0)*16777216 end
    used[row]=count
end
local function engine(options)
    options=options or {};local arena,roots=assert(P.NewArena(identity.arenaGeneration));local pages={};local calls=0;local time=100
    local publications={}
    local dep={identity=identity,publish=function(kind,wire)publications[kind]=wire;return true end,
        compile=options.compile or function(source)return loadstring(source,"=mailbox_fixture")end,
        started=options.started,finished=options.finished,reload=options.reload,
        execute=options.execute or function(fn,_,done)calls=calls+1;local ok,value=pcall(fn);done(ok,value,{resourcesReleased=true});return {}end,
        clock=function()return time end,challenge=function()return string.rep(string.char(187),16)end,
        encode=options.encode or ns.CaptureWriter.Encode,actor=function()if options.noActor then return nil end;return {guid=identity.actorGUID,character=identity.character,realm=identity.realm}end,
        publishPage=function(n,body)pages[n]=body end,clearPages=function()for n in pairs(pages)do pages[n]=nil end end}
    local e=assert(P.Create(dep));assert(e.BindIdentity(identity));assert(e.Enable())
    local function command(source,opts)
        opts=opts or {};opts.challenge=opts.challenge or e.Snapshot().readyChallenge
        if #opts.challenge~=32 then opts.challenge=string.rep("0",32) end
        local wire=header(P.Kind.frame,source,opts);setRow(arena.command,wire);assert(e.Poll(arena,8))
        local ticks=0
        while e.HasPendingValidation() do assert(e.Poll(arena,8));ticks=ticks+1;assert(ticks<10000,"unbounded validation") end
        return wire
    end
    local function ack()
        local snap=e.Snapshot();return assert(P.ResultAckBytes(snap.request.requestId,snap.request.requestSHA256,snap.terminal))
    end
    return {engine=e,arena=arena,roots=roots,pages=pages,publications=publications,command=command,ack=ack,calls=function()return calls end,
        clock=function(v)time=v end,writeStop=function(kind,payload,opts)setRow(arena.stop,header(kind,payload,opts));return e.Poll(arena,8)end}
end
return {ns=ns,P=P,S=S,identity=identity,header=header,setRow=setRow,engine=engine,raw=raw,u32=u32,u64=u64,Z16=Z16,Z32=Z32}
