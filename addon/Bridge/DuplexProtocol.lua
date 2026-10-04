local _, ns = ...

-- Duplex wire primitives and private single-request state machine. Public inbox
-- tables are transport only: all accepted data is copied into private strings.
local SHA = ns.SHA256
local M = 4294967296
local HEADER, FRAME_BYTES, CONTROL_BYTES = 320, 1048576, 1024
local REQUEST_BYTES, FRAME_COUNT, FRAME_SLOTS = 1048576, 1, 1
local ZERO16, ZERO32 = string.rep("\0",16), string.rep("\0",32)
local MAGIC, SEND_MAGIC = "LYCMBX01", "LYCMSB01"
local KIND = {bind=1,frame=2,commit=3,cancel=4,close=5,resultAck=6,reload=7,lease=8,repair=9}
local KINDBYID = {[1]="bind",[2]="frame",[3]="commit",[4]="cancel",[5]="close",[6]="resultAck",[7]="reload",[8]="lease",[9]="repair"}
local function safe(v)return not (issecretvalue and issecretvalue(v)) end
local function u32le(n)
    if not safe(n) or type(n)~="number" or n<0 or n>=M or n%1~=0 then return nil end
    return string.char(n%256,math.floor(n/256)%256,math.floor(n/65536)%256,math.floor(n/16777216)%256)
end
local function read32le(s,p)
    local a,b,c,d=s:byte(p,p+3);if not d then return nil end
    return ((d*256+c)*256+b)*256+a
end
local function read64le(s,p)
    local lo,hi=read32le(s,p),read32le(s,p+4);if not hi then return nil end
    return hi,lo
end
local function tokenRaw(hex,bytes)
    if not safe(hex) or type(hex)~="string" or #hex~=bytes*2 or hex:find("[^0-9a-f]") then return nil end
    return (hex:gsub("..",function(pair)return string.char(tonumber(pair,16))end))
end
local function hex(raw)return SHA.Hex(raw)end
local function uint64String(hi,lo)
    -- Heartbeats, status sequences and ordinary fences fit in the low word.
    -- Every uint32 is exact in Lua's double; avoid the full uint64 conversion's
    -- temporary tables on each idle status publication.
    if hi==0 then return string.format("%.0f",lo) end
    local bytes={math.floor(hi/16777216)%256,math.floor(hi/65536)%256,math.floor(hi/256)%256,hi%256,math.floor(lo/16777216)%256,math.floor(lo/65536)%256,math.floor(lo/256)%256,lo%256}
    local digits="0"
    for i=1,8 do local carry=bytes[i];local out={};for j=#digits,1,-1 do local n=(digits:byte(j)-48)*256+carry;out[#out+1]=string.char(48+n%10);carry=math.floor(n/10) end
        while carry>0 do out[#out+1]=string.char(48+carry%10);carry=math.floor(carry/10) end
        local rev={};for j=#out,1,-1 do rev[#rev+1]=out[j] end;digits=table.concat(rev):gsub("^0+","");if digits=="" then digits="0" end end
    return digits
end
local function numberCell(v)
    return safe(v) and type(v)=="number" and v>=0 and v<M and v%1==0
end
local function arrayShape(t,n)
    if not safe(t) or type(t)~="table" or getmetatable(t)~=nil then return false end
    for i=1,n do if not numberCell(rawget(t,i)) then return false end end
    return rawget(t,n+1)==nil
end
local function wordsToBytes(words,first,count)
    -- Copy through bounded 4 KiB scratch blocks, rather than retaining one
    -- temporary table/string per word of the complete 1 MiB row.
    local out={}
    for at=first,first+count-1,1024 do
        local block={}
        for i=at,math.min(at+1023,first+count-1) do
            local v=rawget(words,i);if not numberCell(v) then return nil,"duplex_cell_invalid" end
            block[#block+1]=string.char(v%256,math.floor(v/256)%256,math.floor(v/65536)%256,math.floor(v/16777216)%256)
        end
        out[#out+1]=table.concat(block)
    end
    return table.concat(out)
end
local function bytesToWords(bytes)
    local out={}
    for i=1,#bytes,4 do out[#out+1]=read32le(bytes,i) end
    return out
end
local function u64word(hi,lo)return u32le(lo)..u32le(hi)end
local function zeroRange(bytes,first,last)
    return bytes:sub(1,first-1)..string.rep("\0",last-first+1)..bytes:sub(last+1)
end
local function fail(reason)return nil,reason end

local API={HeaderBytes=HEADER,FrameBytes=FRAME_BYTES,ControlBytes=CONTROL_BYTES,MaxSource=REQUEST_BYTES,MaxFrames=FRAME_COUNT,FrameSlots=FRAME_SLOTS,SendboxMagic=SEND_MAGIC,Kind=KIND}
function API.NewArena(generation)
    if type(generation)~="string" or #generation~=32 or generation:find("[^0-9a-f]") then return fail("duplex_arena_generation") end
    -- A published leaf must not be resized or replaced by ordinary Lua code.
    -- The native publisher still verifies the frozen flag before each write.
    local freeze=table.freeze
    local isfrozen=table.isfrozen
    if type(freeze)~="function" or type(isfrozen)~="function" then return fail("duplex_frozen_arrays_unavailable") end
    local function cells(n)
        local a={};for i=1,n do a[i]=0 end
        local ok=pcall(freeze,a)
        if not ok or isfrozen(a)~=true then return nil,"duplex_frozen_array_failed" end
        return a
    end
    local command,err=cells(80+FRAME_BYTES/4);if not command then return fail(err) end
    local stop;stop,err=cells(80+CONTROL_BYTES/4);if not stop then return fail(err) end
    local roots={command=command,stop=stop};local calibration={0,1,4294967295,0.125,-13.5,7654321}
    local ok=pcall(freeze,calibration)
    if not ok or isfrozen(calibration)~=true then return fail("duplex_frozen_calibration_failed") end
    roots.calibration=calibration
    return {schema="lycheedev.mailbox.v1",generation=generation,calibration=calibration,command=command,stop=stop},roots
end

-- Converts 80 header words into bytes, rejecting secret and non-integer cells.
function API.ReadHeader(words)
    -- The complete shape and zero tail are checked when a new publication is
    -- copied. A full 1 MiB scan before each header read would double the work.
    if not safe(words) or type(words)~="table" or getmetatable(words)~=nil
        or #words~=80+FRAME_BYTES/4 or rawget(words,81+FRAME_BYTES/4)~=nil then return fail("duplex_frame_shape") end
    return wordsToBytes(words,1,80)
end
function API.ReadControl(words)
    if not arrayShape(words,80+256) then return fail("duplex_control_shape") end
    return wordsToBytes(words,1,80)
end
function API.ReadPayload(words,count,limit)
    if not numberCell(count) or count>limit or count%4~=0 then return fail("duplex_payload_length") end
    local bytes,err=wordsToBytes(words,81,count/4)
    if not bytes then return nil,err end
    return bytes
end

function API.DecodeHeader(raw, expectedKind)
    if type(raw)~="string" or #raw~=HEADER or raw:sub(1,8)~=MAGIC then return fail("duplex_header_invalid") end
    local kind=read32le(raw,9)
    if not KINDBYID[kind] or expectedKind and kind~=expectedKind then return fail("duplex_kind_invalid") end
    local reserved=raw:sub(313,320)
    if reserved~=string.rep("\0",8) then return fail("duplex_reserved_nonzero") end
    local beginHi,beginLo=read64le(raw,297);local endHi,endLo=read64le(raw,305)
    if (beginHi==0 and beginLo==0) or beginLo%2==1 or beginHi~=endHi or beginLo~=endLo then return fail("duplex_publication_unstable") end
    local frameHash=raw:sub(233,264)
    local headerHash=raw:sub(265,296)
    local normalized=zeroRange(raw,265,312)
    local computed,err=SHA.Digest("LYCMBX/header/v1\0"..normalized)
    if not computed then return nil,err end
    if computed~=headerHash then return fail("duplex_header_checksum") end
    local publicationHi,publicationLo=read64le(raw,149)
    local fenceLo,fenceHi=read32le(raw,141),read32le(raw,145)
    local attemptLo,attemptHi=read32le(raw,165),read32le(raw,169)
    if publicationHi==0 and publicationLo==0 or fenceHi==0 and fenceLo==0 or attemptHi==0 and attemptLo==0 then return fail("duplex_header_sequence") end
    local payloadBytes,totalBytes=read32le(raw,185),read32le(raw,189)
    local frameIndex,frameCount=read32le(raw,193),read32le(raw,197)
    if payloadBytes==nil or totalBytes==nil or frameIndex==nil or frameCount==nil then return fail("duplex_header_invalid") end
    if kind==KIND.frame and (totalBytes>REQUEST_BYTES or payloadBytes>FRAME_BYTES or frameCount<1 or frameCount>FRAME_COUNT or frameIndex<1 or frameIndex>frameCount) then return fail("duplex_frame_bounds") end
    if kind==KIND.frame then
        local seqLo,seqHi=read32le(raw,157),read32le(raw,161)
        local frameTotal=math.max(1,math.ceil(totalBytes/FRAME_BYTES))
        if seqLo==0 and seqHi==0 or read32le(raw,181)<1 or read32le(raw,181)>120000 or frameCount~=frameTotal then return fail("duplex_frame_manifest") end
    elseif frameIndex~=0 or frameCount~=0 then return fail("duplex_control_shape") end
    if raw:sub(109,124)==ZERO16 then return fail("duplex_message_id") end
    return {raw=raw,kind=kind,kindName=KINDBYID[kind],laneName=(kind==KIND.bind or kind==KIND.repair) and "bindResume" or KINDBYID[kind],runtime=raw:sub(13,28),arena=raw:sub(29,44),session=raw:sub(45,60),owner=raw:sub(61,76),actorBinding=raw:sub(77,92),requestId=raw:sub(93,108),messageId=raw:sub(109,124),challenge=raw:sub(125,140),fenceLo=fenceLo,fenceHi=fenceHi,publicationSeqLo=publicationLo,publicationSeqHi=publicationHi,requestSeqLo=read32le(raw,157),requestSeqHi=read32le(raw,161),attemptLo=attemptLo,attemptHi=attemptHi,utcLo=read32le(raw,173),utcHi=read32le(raw,177),budget=read32le(raw,181),payloadBytes=payloadBytes,totalBytes=totalBytes,frameIndex=frameIndex,frameCount=frameCount,requestSHA=raw:sub(201,232),frameSHA=frameHash,headerSHA=headerHash}
end

function API.VerifyFrame(rawHeader,payload,expected)
    local h,err=API.DecodeHeader(rawHeader,KIND.frame);if not h then return nil,err end
    if type(payload)~="string" or #payload~=h.payloadBytes then return fail("duplex_frame_length") end
    if h.frameIndex~=1 or h.frameCount~=1 or h.payloadBytes~=h.totalBytes then return fail("duplex_frame_bounds") end
    local digest;digest,err=API.RequestDigest(h.requestId,h.actorBinding,h.budget,h.utcHi,h.utcLo,h.totalBytes,payload)
    if not digest then return nil,err end
    if digest~=h.requestSHA then return fail("duplex_request_checksum") end
    h.previousResultAckSHA=h.frameSHA
    if expected then
        for _,key in ipairs({"runtime","arena","session","owner","actorBinding","requestId","requestSeqHi","requestSeqLo","attemptHi","attemptLo","totalBytes","frameCount"}) do
            if expected[key]~=nil and expected[key]~=h[key] then return fail("duplex_frame_identity") end
        end
    end
    return h
end
function API.RequestDigest(requestId,actorBinding,budget,utcHi,utcLo,total,source)
    if type(source)~="string" or #source~=total or #source>REQUEST_BYTES then return fail("duplex_request_length") end
    if type(requestId)~="string" or #requestId~=16 or type(actorBinding)~="string" or #actorBinding~=16 then return fail("duplex_request_identity") end
    local body="LYCMBX/request/v1\0"..requestId..actorBinding..u32le(budget)..u64word(utcHi,utcLo)..u32le(total)..source
    return SHA.Digest(body)
end
function API.VerifyResultAck(payload,requestId,requestSHA,terminal)
    local id=tokenRaw(requestId,16);local digest=tokenRaw(requestSHA,32)
    local result=type(terminal)=="table" and tokenRaw(terminal.resultSHA256,32)
    local state=terminal and (terminal.outcome=="success" and 1 or terminal.outcome=="failed" and 2 or terminal.outcome=="cancelled" and 3)
    if not id or not digest or not result or not numberCell(terminal.resultBytes) or not numberCell(terminal.pages)
        or type(payload)~="string" or #payload~=92 then return fail("duplex_ack_shape") end
    if payload:sub(1,16)~=id or payload:sub(17,48)~=digest or read32le(payload,49)~=state
        or payload:sub(53,84)~=result or read32le(payload,85)~=terminal.resultBytes or read32le(payload,89)~=terminal.pages then return fail("duplex_ack_mismatch") end
    return true
end

function API.Sendbox(record,limit)
    local text,err=ns.CaptureWriter.Encode(record,limit or 65536)
    if not text then return nil,err or "duplex_sendbox_encode" end
    local digest;digest,err=SHA.Digest(text);if not digest then return nil,err end
    return SEND_MAGIC..u32le(#text)..digest..text
end
function API.ParseSendbox(wire,limit)
    if type(wire)~="string" or #wire<44 or wire:sub(1,8)~=SEND_MAGIC then return fail("duplex_sendbox_invalid") end
    local n=read32le(wire,9);if not n or n>(limit or 65536) or #wire~=44+n then return fail("duplex_sendbox_length") end
    local text=wire:sub(45);local digest=SHA.Digest(text)
    if digest~=wire:sub(13,44) then return fail("duplex_sendbox_checksum") end
    return text
end
function API.ResultPage(requestId,requestSHA,index,count,body)
    local id=tokenRaw(requestId,16);local digest=tokenRaw(requestSHA,32)
    if not id or not digest or not numberCell(index) or index<1 or index>32 or not numberCell(count) or count<1 or count>32
        or index>count or type(body)~="string" or #body>16384 then return fail("duplex_result_page_invalid") end
    local pageSHA=SHA.Digest(body)
    return "LYCMRP01"..id..digest..u32le(index)..u32le(count)..u32le(#body)..pageSHA..body
end

local function detached(t)
    local out={};for k,v in pairs(t) do if type(v)=="table" then out[k]=detached(v) else out[k]=v end end;return out
end
function API.ResultAckBytes(requestId,requestSHA,terminal)
    local id,digest=tokenRaw(requestId,16),tokenRaw(requestSHA,32)
    local result=terminal and tokenRaw(terminal.resultSHA256,32)
    local state=terminal and (terminal.outcome=="success" and 1 or terminal.outcome=="failed" and 2 or terminal.outcome=="cancelled" and 3)
    if not id or not digest or not result or not state or not numberCell(terminal.resultBytes) or not numberCell(terminal.pages) then return fail("duplex_ack_shape") end
    return id..digest..u32le(state)..result..u32le(terminal.resultBytes)..u32le(terminal.pages)
end
function API.Create(deps)
    if type(deps)~="table" or type(deps.identity)~="table" or type(deps.publish)~="function" or type(deps.execute)~="function"
        or type(deps.compile)~="function" or type(deps.clock)~="function" or type(deps.encode)~="function" or type(deps.challenge)~="function" then return fail("duplex_dependencies_invalid") end
    local p={identity=detached(deps.identity),disabled=true,seqHi=0,seqLo=0,fenceHi=0,fenceLo=0,admission=0,statusSequence=0,heartbeat=1,receipts={}}
    local observed=setmetatable({}, {__mode="k"})
    local failures=setmetatable({}, {__mode="k"})
    local api={}
    local function actorReady()
        local ok,a=pcall(function()return deps.actor and deps.actor()end)
        local i=p.identity
        return ok and a and a.guid==i.actorGUID and a.character==i.character and a.realm==i.realm or false
    end
    local function refreshActor()
        local available=actorReady()
        if p.lastActorReady~=nil and p.lastActorReady~=available then
            p.readyRaw=nil;p.readyAt=nil
            if p.validation then p.validation=nil;p.lastFailure="duplex_actor_changed_before_acceptance" end
        end
        p.lastActorReady=available
        return available
    end
    local function now()
        local ok,n=pcall(deps.clock)
        if ok and safe(n) and type(n)=="number" and n==n and n>=0 and n<math.huge then return n end
    end
    local function eligible()
        return not p.disabled and not p.closing and not p.repair and not p.quarantined and not p.reload and not p.reloadPending
            and (not p.request or p.request.state=="terminal" and p.terminal and p.terminal.resourcesReleased==true)
    end
    local function issueReady(available)
        if p.readyRaw or not eligible() then return end
        if available==nil then available=actorReady() end
        if not available then return end
        local clock=now();if not clock then p.lastFailure="duplex_clock_unavailable";return end
        p.admission=p.admission+1
        local ok,raw=pcall(deps.challenge,{admissionSequence=p.admission})
        if not ok or type(raw)~="string" or #raw~=16 then p.lastFailure="duplex_challenge_unavailable";return end
        -- Even a deterministic fixture source must yield a different token for
        -- every admission; consumption can never be undone by token reuse.
        p.readyRaw=SHA.Digest("LYCMBX/admission/v1\0"..tokenRaw(p.identity.runtime,16)..tokenRaw(p.identity.arenaGeneration,16)..u64word(0,p.admission)..raw):sub(1,16)
        p.readyAt=clock
    end
    local function phase()
        if p.quarantined then return "quarantined" end
        if p.closing then return p.request and "closing" or "closed" end
        if p.validation then return "validating" end
        if p.request then return p.request.state=="terminal" and "result_pending" or p.request.state end
        return "ready_unbound"
    end
    local function record()
        local available=refreshActor();issueReady(available);p.statusSequence=p.statusSequence+1
        local i,r,t=p.identity,p.request,p.terminal
        local ready=eligible() and not p.validation and p.readyRaw~=nil and available
        return {schema="lycheedev.mailbox.v1",layoutId="single-command-row-v1",runtimeToken=i.runtime,arenaGeneration=i.arenaGeneration,
            sessionToken=p.session or string.rep("0",32),ownerToken=p.owner or string.rep("0",32),actorBindingId=i.actorBindingId,
            fence=uint64String(p.fenceHi,p.fenceLo),actorGUID=i.actorGUID,character=i.character,realm=i.realm,build=i.build,product=i.product,release=i.release,
            resourcesReleased=not r or t and t.resourcesReleased==true or false,phase=phase(),ready=ready,businessReady=ready,
            transportReady=not p.disabled and not p.quarantined and not p.closing and not p.repair,actorReady=available,controlReady=not p.disabled and not p.quarantined,
            readyChallenge=p.readyRaw and hex(p.readyRaw) or "",admissionSequence=uint64String(0,p.admission),
            statusSequence=uint64String(0,p.statusSequence),heartbeat=uint64String(0,p.heartbeat),receipts=detached(p.receipts),
            request=r and {requestId=r.requestId,requestSHA256=r.digest,requestSeq=uint64String(r.seqHi,r.seqLo),transportAttempt=uint64String(r.attemptHi,r.attemptLo),totalBytes=r.totalBytes,acceptedFrames={1},challenge=r.challenge,notStarted=r.executionStarted~=true} or nil,
            terminal=t and {requestId=r.requestId,requestSHA256=r.digest,state=t.outcome,sha256=t.resultSHA256,bytes=t.resultBytes,pages=t.pages,pageSHA256=t.pageSHA256,executionStarted=t.executionStarted,effects=t.effects,resourcesReleased=t.resourcesReleased,failureCode=t.failureCode} or nil,
            released=p.released and detached(p.released) or nil,closedAdmission=p.closedAdmission==true,repair=p.repair and detached(p.repair) or nil,
            reload=p.reload and {messageId=p.reload.messageId,challenge=p.reload.challenge} or nil,
            validation=p.validation and {requestId=hex(p.validation.header.requestId),requestSHA256=hex(p.validation.header.requestSHA),totalBytes=p.validation.header.totalBytes,
                copiedBytes=math.min(p.validation.header.payloadBytes,p.validation.copiedCells*4),hashedBytes=p.validation.hashedBytes or 0} or nil}
    end
    local function publish(kind)
        local wire,err=API.Sendbox(record(),65536);if not wire then return nil,err end
        local ok,value=pcall(deps.publish,kind or "status",wire)
        if not ok or value==false then return fail("duplex_sendbox_publish") end
        if kind=="receipts" then
            ok,value=pcall(deps.publish,"status",wire)
            if not ok or value==false then return fail("duplex_sendbox_publish") end
        end
        return true
    end
    local function receipt(h,state,extra)
        local v={messageId=hex(h.messageId),requestId=hex(h.requestId),requestSHA256=hex(h.requestSHA),state=state}
        if extra then for k,x in pairs(extra) do v[k]=x end end
        p.receipts.stop=v
        return publish("receipts")
    end
    local function identity(h,unbound)
        local i=p.identity
        if h.runtime~=tokenRaw(i.runtime,16) or h.arena~=tokenRaw(i.arenaGeneration,16) or h.actorBinding~=tokenRaw(i.actorBindingId,16)
            or h.owner==ZERO16 or h.session==ZERO16 then return false end
        if not p.owner then return unbound end
        if unbound and p.closedAdmission then
            local hi,lo=p.fenceHi,p.fenceLo
            if lo==4294967295 then hi=hi+1;lo=0 else lo=lo+1 end
            return hi<4294967296 and h.owner~=tokenRaw(p.owner,16) and h.session~=tokenRaw(p.session,16)
                and h.fenceHi==hi and h.fenceLo==lo and h.requestSeqHi==0 and h.requestSeqLo==1
        end
        return h.owner==tokenRaw(p.owner,16) and h.session==tokenRaw(p.session,16) and h.fenceHi==p.fenceHi and h.fenceLo==p.fenceLo
    end
    local function bind(h)
        if not p.owner or p.closedAdmission then
            p.owner=hex(h.owner);p.session=hex(h.session);p.fenceHi=h.fenceHi;p.fenceLo=h.fenceLo
            if p.closedAdmission then
                p.seqHi=0;p.seqLo=0;p.lastRequestRaw=nil;p.receipts={};p.stopSeqHi=nil;p.stopSeqLo=nil;p.stopFingerprint=nil
                observed=setmetatable({}, {__mode="k"});p.closedAdmission=nil;p.rearm=nil
            end
        end
    end
    local function nextSequence(h)
        if not p.owner or p.closedAdmission then return h.requestSeqHi==0 and h.requestSeqLo==1 end
        local hi,lo=p.seqHi,p.seqLo
        if lo==4294967295 then hi=hi+1;lo=0 else lo=lo+1 end
        return hi<4294967296 and h.requestSeqHi==hi and h.requestSeqLo==lo
    end
    local function terminalize(r,success,value,meta,started,code)
        if p.request~=r or r.state=="terminal" or r.state=="sealing" then return end
        r.state="sealing";r.fn=nil;r.execution=nil;r.executionStarted=started==true
        if started and deps.finished then pcall(deps.finished,r.requestId) end
        publish("status")
        meta=meta or {}
        local released=meta.resourcesReleased~=false
        local body={ok=success==true,resourcesReleased=released,logs=meta.logs}
        if success then body.result=value else body.error=value end
        local json=deps.encode(body,524288)
        if not json then success=false;code="result_encoding_error";json='{"ok":false,"error":"result_encoding_error","resourcesReleased":false}';released=false end
        local pages,hashes={},{}
        for at=1,#json,16384 do local page=json:sub(at,at+16383);pages[#pages+1]=page;hashes[#hashes+1]=hex(SHA.Digest(page)) end
        if #pages==0 then pages[1]="";hashes[1]=hex(SHA.Digest("")) end
        r.state="terminal";r.fn=nil;r.execution=nil;r.executionStarted=started==true
        p.terminal={outcome=meta.cancelled and "cancelled" or success and "success" or "failed",executionStarted=started==true,
            failureCode=code or not success and (meta.cancelled and "cancelled" or "probe_failed") or nil,
            effects=started and "may_have_occurred" or "none_started",resourcesReleased=released,resultSHA256=hex(SHA.Digest(json)),resultBytes=#json,pages=#pages,pageSHA256=hashes}
        if deps.publishPage then for n,page in ipairs(pages) do pcall(deps.publishPage,n,page) end end
        publish("terminal")
    end
    local function readRow(words,capacity,command)
        if not safe(words) or type(words)~="table" or getmetatable(words)~=nil or #words~=80+capacity/4 or rawget(words,81+capacity/4)~=nil then return fail("duplex_lane_shape") end
        local hdr,err=wordsToBytes(words,1,80);if not hdr then return nil,err end
        local h;h,err=API.DecodeHeader(hdr,command and KIND.frame or nil);if not h then return nil,err end
        if h.payloadBytes>capacity then return fail("duplex_payload_length") end
        local payload;payload,err=wordsToBytes(words,81,math.ceil(h.payloadBytes/4));if not payload then return nil,err end
        if h.payloadBytes%4~=0 and payload:sub(h.payloadBytes+1)~=string.rep("\0",4-h.payloadBytes%4) then return fail("duplex_frame_padding") end
        payload=payload:sub(1,h.payloadBytes)
        -- Equality with zero also rejects strings, booleans and objects; after
        -- the secret guard it avoids repeating integer arithmetic for every
        -- unused cell of the fixed 1 MiB tail.
        for n=81+math.ceil(h.payloadBytes/4),80+capacity/4 do local v=rawget(words,n);if not safe(v) or v~=0 then return fail("duplex_frame_padding") end end
        local again;again,err=wordsToBytes(words,1,80);if not again or again~=hdr then return fail("duplex_frame_changed") end
        if command then local verified,why=API.VerifyFrame(hdr,payload);if not verified then return nil,why end;return verified,payload end
        if h.kind==KIND.frame or SHA.Digest("LYCMBX/frame/v1\0"..hdr:sub(1,232)..payload)~=h.frameSHA then return fail("duplex_control_checksum") end
        return h,payload
    end
    local function candidate(row)
        if type(row)~="table" then return true end
        -- An all-zero or malformed magic is never a command. Do not allocate
        -- decode work every idle tick; a completing host write remains visible.
        if rawget(row,1)~=read32le(MAGIC,1) or rawget(row,2)~=read32le(MAGIC,5) then return false end
        local a,b,c,d=rawget(row,75),rawget(row,76),rawget(row,77),rawget(row,78)
        if not numberCell(a) or not numberCell(b) or not numberCell(c) or not numberCell(d) then return true end
        if a==0 and b==0 or a%2~=0 or a~=c or b~=d then return false end
        local old=observed[row]
        if old and old.lo==a and old.hi==b then
            local same=true
            for n=67,74 do if rawget(row,n)~=old[n] then same=false;break end end
            if same then return false end
        end
        local bad=failures[row]
        if bad and bad.lo==a and bad.hi==b then
            local same=true
            for n=67,74 do if rawget(row,n)~=bad[n] then same=false;break end end
            if same and bad.retryAt then local clock=now();if clock and clock<bad.retryAt then return false end end
        else failures[row]=nil end
        local stamp={lo=a,hi=b}
        for n=67,74 do stamp[n]=rawget(row,n) end
        return true,stamp
    end
    local function remember(row,stamp)
        if not stamp then return end
        if rawget(row,75)~=stamp.lo or rawget(row,76)~=stamp.hi or rawget(row,77)~=stamp.lo or rawget(row,78)~=stamp.hi then observed[row]=nil;return end
        for n=67,74 do if rawget(row,n)~=stamp[n] then observed[row]=nil;return end end
        local shadow={lo=stamp.lo,hi=stamp.hi}
        for n=67,74 do shadow[n]=stamp[n] end
        observed[row]=shadow;failures[row]=nil
    end
    local function failed(row,stamp)
        if not stamp then return end
        local bad=failures[row]
        if not bad or bad.lo~=stamp.lo or bad.hi~=stamp.hi then bad={lo=stamp.lo,hi=stamp.hi,count=0};failures[row]=bad end
        bad.count=bad.count+1
        for n=67,74 do bad[n]=rawget(row,n) end
        -- A completed OS write can repair the body without changing marker.
        -- Retest it after bounded backoff; a new header is inspected at once.
        if bad.count>=3 then local clock=now();if clock then bad.retryAt=clock+0.5 end end
    end
    local function newRequest(h)
        return {requestRaw=h.requestId,requestId=hex(h.requestId),digestRaw=h.requestSHA,digest=hex(h.requestSHA),seqHi=h.requestSeqHi,seqLo=h.requestSeqLo,
            attemptHi=h.attemptHi,attemptLo=h.attemptLo,totalBytes=h.totalBytes,budget=h.budget,challenge=hex(h.challenge),previousResultAckSHA=h.previousResultAckSHA or ZERO32,executionStarted=false,state="running"}
    end
    local function retire()
        local r,t=p.request,p.terminal
        p.released={requestId=r.requestId,requestSHA256=r.digest,requestSeq=uint64String(r.seqHi,r.seqLo),
            totalBytes=r.totalBytes,outcome=t and t.outcome or nil,executionStarted=t and t.executionStarted or false,
            failureCode=t and t.failureCode or nil}
        p.request=nil;p.terminal=nil
        if deps.clearPages then pcall(deps.clearPages) end
    end
    local function admissionChecks(h)
        if not identity(h,true) then return nil,"duplex_frame_identity",true end
        if not actorReady() then return fail("duplex_frame_identity") end
        local clock=now()
        if not eligible() or h.challenge~=p.readyRaw then return nil,"duplex_challenge_mismatch",true end
        if not clock or clock<p.readyAt then return fail("duplex_clock_unavailable") end
        if h.requestId==ZERO16 or not nextSequence(h)
            or h.requestId==p.lastRequestRaw then return nil,"duplex_request_sequence",true end
        if p.terminal then
            local ack=API.ResultAckBytes(p.request.requestId,p.request.digest,p.terminal)
            if not ack or h.previousResultAckSHA~=SHA.Digest("LYCMBX/result-ack/v1\0"..ack) then return nil,"duplex_ack_mismatch",true end
        elseif h.previousResultAckSHA~=ZERO32 then return nil,"duplex_ack_unexpected",true end
        return true
    end
    local function admit(h,source)
        local checked,why,rejected=admissionChecks(h);if not checked then return nil,why,rejected end
        -- All validations finish before either the old result or nonce changes.
        if p.terminal then retire() end
        bind(h);p.readyRaw=nil;p.readyAt=nil;p.seqHi=h.requestSeqHi;p.seqLo=h.requestSeqLo;p.lastRequestRaw=h.requestId
        local r=newRequest(h);p.request=r;p.lastFailure=nil
        local ok,fn=pcall(deps.compile,source);source=nil
        if not ok or type(fn)~="function" then terminalize(r,false,"compile_error",{resourcesReleased=true},false,"compile_error");return true end
        r.fn=fn;r.executionStarted=true
        publish("accepted")
        if deps.started then pcall(deps.started,r.requestId) end
        local executed,handle=pcall(deps.execute,fn,math.min(120,math.max(1,math.ceil(r.budget/1000))),function(success,value,meta)
            terminalize(r,success,value,meta,true)
        end)
        if not executed then terminalize(r,false,"executor_error",{resourcesReleased=false},true,"executor_error")
        elseif r.state=="running" then r.execution=handle end
        return true
    end
    local function commandRow(row,stamp)
        local raw,err=API.ReadHeader(row);if not raw then return nil,err end
        local h;h,err=API.DecodeHeader(raw,KIND.frame);if not h then return nil,err end
        if h.frameIndex~=1 or h.frameCount~=1 or h.payloadBytes~=h.totalBytes then return fail("duplex_frame_bounds") end
        h.previousResultAckSHA=h.frameSHA
        refreshActor()
        local checked,why,rejected=admissionChecks(h);if not checked then return nil,why,rejected end
        if h.payloadBytes<=512 then
            local verified,source=readRow(row,FRAME_BYTES,true);if not verified then return nil,source end
            return admit(verified,source)
        end
        p.validation={row=row,stamp=stamp,raw=raw,header=h,parts={},copiedCells=0,tailCell=81+math.ceil(h.payloadBytes/4),hashedBytes=0}
        publish("status")
        return false
    end
    local function advanceValidation()
        local j=p.validation;if not j then return false end
        local h=j.header
        refreshActor()
        local function discard(reason)
            p.validation=nil;p.lastFailure=reason;failed(j.row,j.stamp);publish("status")
            return nil,reason
        end
        -- Admission is still unconsumed. Exact stop revocation, actor changes
        -- or any changed row header discard this private candidate immediately.
        if not actorReady() then
            p.validation=nil;p.readyRaw=nil;p.readyAt=nil;p.lastFailure="duplex_actor_changed_before_acceptance";remember(j.row,j.stamp);publish("status");return false
        end
        if not eligible() or h.challenge~=p.readyRaw or not identity(h,true) then p.validation=nil;publish("status");return false end
        local current,why=API.ReadHeader(j.row)
        if not current or current~=j.raw then return discard(why or "duplex_frame_changed") end
        local sourceCells=math.ceil(h.payloadBytes/4)
        if j.copiedCells<sourceCells then
            local count=math.min(2048,sourceCells-j.copiedCells)
            local bytes;bytes,why=wordsToBytes(j.row,81+j.copiedCells,count)
            if not bytes then return discard(why) end
            j.parts[#j.parts+1]=bytes;j.copiedCells=j.copiedCells+count
            return false
        end
        if j.tailCell<=80+FRAME_BYTES/4 then
            local finish=math.min(j.tailCell+8191,80+FRAME_BYTES/4)
            for n=j.tailCell,finish do local value=rawget(j.row,n);if not safe(value) or value~=0 then return discard("duplex_frame_padding") end end
            j.tailCell=finish+1;return false
        end
        if not j.source then
            local source=table.concat(j.parts);j.parts=nil
            if source:sub(h.payloadBytes+1)~=string.rep("\0",#source-h.payloadBytes) then return discard("duplex_frame_padding") end
            j.source=source:sub(1,h.payloadBytes);source=nil
            j.hash=SHA.New()
            local prefix="LYCMBX/request/v1\0"..h.requestId..h.actorBinding..u32le(h.budget)..u64word(h.utcHi,h.utcLo)..u32le(h.totalBytes)
            local ok;ok,why=SHA.Update(j.hash,prefix);if not ok then return discard(why) end
            return false
        end
        if j.hashedBytes<#j.source then
            local count=SHA.NativeBit and 2048 or 256
            local chunk=j.source:sub(j.hashedBytes+1,j.hashedBytes+count)
            local ok;ok,why=SHA.Update(j.hash,chunk);if not ok then return discard(why) end
            j.hashedBytes=j.hashedBytes+#chunk;return false
        end
        local digest;digest,why=SHA.Final(j.hash);j.hash=nil
        if not digest or digest~=h.requestSHA then return discard(why or "duplex_request_checksum") end
        -- The private copy has now passed the same full-source SHA as the
        -- single-callback path. Atomically recheck and consume its admission.
        p.validation=nil
        local ok,err,rejected=admit(h,j.source);j.source=nil
        if ok or rejected then remember(j.row,j.stamp) end
        if not ok then p.lastFailure=err end
        return ok,err
    end
    local function stopRow(row)
        local h,payload=readRow(row,CONTROL_BYTES,false);if not h then return nil,payload end
        if h.kind~=KIND.cancel and h.kind~=KIND.close and h.kind~=KIND.repair and h.kind~=KIND.reload and h.kind~=KIND.lease then return fail("duplex_control_kind") end
        if not identity(h,h.kind==KIND.cancel or h.kind==KIND.close) then return fail("duplex_control_identity") end
        local fingerprint=hex(SHA.Digest(h.raw..payload))
        local old=p.receipts.stop
        if old and old.messageId==hex(h.messageId) then
            if fingerprint~=p.stopFingerprint then return fail("duplex_control_replay_changed") end
            return true
        end
        if p.stopSeqHi and (h.publicationSeqHi<p.stopSeqHi or h.publicationSeqHi==p.stopSeqHi and h.publicationSeqLo<=p.stopSeqLo) then return fail("duplex_control_sequence") end
        if h.kind==KIND.reload or h.kind==KIND.lease then
            if p.request or p.terminal or p.validation or h.requestId~=ZERO16 or h.requestSHA~=ZERO32
                or h.requestSeqHi~=0 or h.requestSeqLo~=0 or h.budget~=0 or h.totalBytes~=0 then return fail("duplex_reload_blocked") end
            local clock=now();if not clock then return fail("duplex_clock_unavailable") end
            if h.kind==KIND.reload then
                if #payload~=0 or p.reloadPending or type(deps.reload)~="function" then return fail("duplex_reload_unavailable") end
                local permitted,available=pcall(deps.reload);if not permitted or not available then return fail("duplex_reload_unavailable") end
                local token=SHA.Digest("LYCMBX/reload/v1\0"..h.messageId..h.runtime..u64word(0,math.floor(clock*1000))):sub(1,16)
                bind(h);p.readyRaw=nil;p.readyAt=nil;p.reload={messageId=hex(h.messageId),messageRaw=h.messageId,challenge=hex(token),challengeRaw=token,expires=clock+30}
            else
                local reload=p.reload
                if not reload or clock>=reload.expires or h.challenge~=reload.challengeRaw or payload~=reload.messageRaw then return fail("duplex_reload_lease_rejected") end
                p.reload=nil;p.reloadPending=true
            end
            p.stopSeqHi=h.publicationSeqHi;p.stopSeqLo=h.publicationSeqLo;p.stopFingerprint=fingerprint
            return receipt(h,"accepted",p.reload and {challenge=p.reload.challenge} or nil)
        elseif h.kind==KIND.repair then
            if #payload~=0 or not p.repair or h.challenge~=tokenRaw(p.repair.challenge,16) then return fail("duplex_repair_mismatch") end
            p.repair.previousWriterDrained=true;p.repaired=true;p.lastFailure=nil
        elseif #payload~=0 and not ((h.kind==KIND.close or h.kind==KIND.cancel) and #payload==92) then return fail("duplex_control_payload_length") end
        local r=p.request
        if h.kind==KIND.close and #payload==92 then
            if not r or r.state~="terminal" or not p.terminal or not p.terminal.resourcesReleased
                or h.requestId~=r.requestRaw or h.requestSHA~=r.digestRaw or not API.VerifyResultAck(payload,r.requestId,r.digest,p.terminal) then return fail("duplex_ack_mismatch") end
            p.closing=true;p.validation=nil;p.readyRaw=nil;p.readyAt=nil;retire();p.rearm=true
        elseif h.kind~=KIND.repair then
            if h.kind==KIND.cancel and r and h.requestId~=r.requestRaw then
                -- An uncertain replacement command may still leave its prior
                -- terminal visible. Its immutable previous ACK is carried by
                -- this independent cancellation, so both old retirement and
                -- the new no-execution record happen together.
                if #payload~=92 or r.state~="terminal" or not p.terminal or not p.terminal.resourcesReleased
                    or not API.VerifyResultAck(payload,r.requestId,r.digest,p.terminal) or not eligible()
                    or h.challenge~=p.readyRaw or h.requestId==ZERO16 or h.requestSHA==ZERO32 or not actorReady()
                    or not nextSequence(h) then return fail("duplex_cancel_unaccepted_mismatch") end
                h.previousResultAckSHA=SHA.Digest("LYCMBX/result-ack/v1\0"..payload)
                retire();bind(h);p.validation=nil;p.readyRaw=nil;p.readyAt=nil;p.seqHi=h.requestSeqHi;p.seqLo=h.requestSeqLo;p.lastRequestRaw=h.requestId
                r=newRequest(h);p.request=r;terminalize(r,false,"cancelled_before_execution",{cancelled=true,resourcesReleased=true},false,"cancelled_before_execution")
            end
            if r then
                if h.requestId~=r.requestRaw or h.requestSHA~=r.digestRaw or h.requestSeqHi~=r.seqHi or h.requestSeqLo~=r.seqLo then return fail("duplex_control_request") end
                if h.kind==KIND.cancel and #payload==92 and SHA.Digest("LYCMBX/result-ack/v1\0"..payload)~=r.previousResultAckSHA then return fail("duplex_ack_mismatch") end
                if h.kind==KIND.close then p.closing=true;p.validation=nil;p.readyRaw=nil;p.readyAt=nil end
                if r.state=="running" then
                    local ok,accepted=pcall(function()return r.execution and r.execution:RequestCancel()end)
                    if not ok or not accepted then p.lastFailure="duplex_cancel_pending" end
                end
            elseif h.requestId~=ZERO16 then
                -- Revocation wins over a late or same-tick command publication.
                if #payload~=0 or h.challenge~=p.readyRaw or h.requestSHA==ZERO32 or not actorReady()
                    or not nextSequence(h) then return fail("duplex_cancel_unaccepted_mismatch") end
                bind(h);p.validation=nil;p.readyRaw=nil;p.readyAt=nil;p.seqHi=h.requestSeqHi;p.seqLo=h.requestSeqLo;p.lastRequestRaw=h.requestId
                if h.kind==KIND.close then p.closing=true end
                r=newRequest(h);p.request=r;terminalize(r,false,"cancelled_before_execution",{cancelled=true,resourcesReleased=true},false,"cancelled_before_execution")
            elseif h.kind==KIND.close and h.requestSHA==ZERO32 then
                bind(h);p.closing=true;p.validation=nil;p.readyRaw=nil;p.readyAt=nil;p.rearm=true
            else return fail("duplex_control_request") end
        end
        p.stopSeqHi=h.publicationSeqHi;p.stopSeqLo=h.publicationSeqLo;p.stopFingerprint=fingerprint
        return receipt(h,h.kind==KIND.repair and "repaired" or p.closing and (p.request and "closing" or "closed") or r and not r.executionStarted and "not_started" or r and r.state=="terminal" and "cancel_too_late" or "cancel_requested",{executionStarted=r and r.executionStarted==true or false})
    end
    function api.BindIdentity(i)
        if type(i)~="table" or not tokenRaw(i.runtime,16) or not tokenRaw(i.arenaGeneration,16) or not tokenRaw(i.actorBindingId,16) then return fail("duplex_identity_invalid") end
        p.identity=detached(i);return true
    end
    function api.Enable()if p.quarantined then return fail("duplex_quarantined") end;if p.disabled then p.disabled=false;refreshActor();issueReady() end;return true end
    function api.Disable()
        if p.request and p.request.state=="running" then
            local ok,accepted=pcall(function()return p.request.execution and p.request.execution:RequestCancel()end)
            if not ok or not accepted or p.request.state=="running" then return fail("duplex_cancel_pending") end
        end
        p.disabled=true;p.validation=nil;p.readyRaw=nil;p.readyAt=nil;return publish("status")
    end
    function api.PublishStatus()return publish("status")end
    function api.Heartbeat()p.heartbeat=p.heartbeat+1;return publish("status")end
    function api.Poll(arena,budget)
        if p.disabled or p.quarantined then return fail("duplex_disabled") end
        budget=budget or 1;if not numberCell(budget) or budget<1 or budget>8 then return fail("duplex_budget_invalid") end
        -- Final ACK is observed as closed before a later poll issues another
        -- admission. The host must observe that exact receipt and retire both
        -- writer intents before it uses this nonce with a new owner/fence.
        if p.rearm and p.closing and not p.request and not p.terminal then
            p.closing=false;p.closedAdmission=true;p.rearm=nil;publish("status")
        end
        if p.reload then local clock=now();if clock and clock>=p.reload.expires then p.reload=nil end end
        issueReady();local processed=0
        local check,stamp=candidate(arena.stop)
        if check then local ok,err=stopRow(arena.stop);if ok then remember(arena.stop,stamp);processed=processed+1 elseif err then p.lastFailure=err;failed(arena.stop,stamp) end end
        if processed<budget and p.validation then
            -- The stop row was inspected first. Batch only private validation
            -- work, with a small wall-clock cap and a hard microstep ceiling.
            -- Without a safe high-resolution clock, retain one step per poll.
            local started
            if type(deps.workClockMillis)=="function" then
                local valid,value=pcall(deps.workClockMillis)
                if valid and safe(value) and type(value)=="number" and value==value and value>=0 and value<math.huge then started=value end
            end
            local steps=0
            repeat
                local ok=advanceValidation();steps=steps+1
                if ok then processed=processed+1 end
                if not p.validation or not started or steps>=64 then break end
                local valid,value=pcall(deps.workClockMillis)
                if not valid or not safe(value) or type(value)~="number" or value~=value or value<started or value-started>=0.75 then break end
            until false
        elseif processed<budget and eligible() then
            check,stamp=candidate(arena.command)
            if check then
                local ok,err,rejected=commandRow(arena.command,stamp)
                if ok or rejected then remember(arena.command,stamp) end
                if ok then processed=processed+1 elseif err then p.lastFailure=err;if not rejected then failed(arena.command,stamp) end end
            end
        end
        return processed
    end
    function api.Quarantine(reason)
        p.disabled=true;p.quarantined=true;p.validation=nil;p.readyRaw=nil;p.lastFailure=reason or "duplex_quarantined"
        local r=p.request
        if r and r.state=="running" and r.execution and type(r.execution.RequestCancel)=="function" then
            -- Cancellation invokes the normal completion only after actual
            -- execution/cleanup ends. Never hide activity merely on quarantine.
            pcall(function()return r.execution:RequestCancel()end)
        end
        return publish("status")
    end
    function api.BeginRepair(generation,raw)
        if not p.owner or p.repair or p.retiredArena or p.request or p.terminal or p.validation or p.closing or not tokenRaw(generation,16) or generation==p.identity.arenaGeneration
            or type(raw)~="string" or #raw~=16 then return fail("duplex_repair_unavailable") end
        p.repair={previousArena=p.identity.arenaGeneration,newArena=generation,challenge=hex(raw),requestId=string.rep("0",32),requestSHA256=string.rep("0",64),
            previousWriterDrained=false,idle=true,noPendingRequest=true,resourcesReleased=true,ledgerRetained=true}
        p.retiredArena=true;p.identity.arenaGeneration=generation;p.readyRaw=nil;p.readyAt=nil;p.repaired=false;return true
    end
    function api.ReleaseRetiredArena()
        if not p.repaired or not p.retiredArena then return fail("duplex_retired_arena_unconfirmed") end
        p.retiredArena=nil;p.repair=nil;issueReady();return publish("status")
    end
    function api.RuntimeState()return p.quarantined==true,p.lastFailure,p.repaired==true end
    function api.HasPendingValidation()return p.validation~=nil end
    function api.HasOwner()return p.owner~=nil end
    function api.TakeReload()if not p.reloadPending then return false end;p.reloadPending=false;return true end
    function api.Snapshot()
        local r=p.request
        local actor=actorReady();local ready=eligible() and not p.validation and p.readyRaw~=nil and actor
        return {identity=detached(p.identity),phase=phase(),request=r and {requestId=r.requestId,requestSHA256=r.digest,requestSeq=uint64String(r.seqHi,r.seqLo),acceptedFrames=1,frameCount=1,receivedBytes=r.totalBytes,totalBytes=r.totalBytes,challenge=r.challenge,executionStarted=r.executionStarted} or nil,
            terminal=p.terminal and detached(p.terminal),receipts=detached(p.receipts),closing=p.closing==true,disabled=p.disabled,quarantined=p.quarantined==true,lastFailure=p.lastFailure,
            ready=ready,businessReady=ready,actorReady=actor,transportReady=not p.disabled and not p.quarantined and not p.closing and not p.repair,controlReady=not p.disabled and not p.quarantined,
            readyChallenge=p.readyRaw and hex(p.readyRaw) or "",admissionSequence=uint64String(0,p.admission),closedAdmission=p.closedAdmission==true,released=p.released and detached(p.released),repair=p.repair and detached(p.repair),repaired=p.repaired==true,retiredArena=p.retiredArena==true,
            validation=p.validation and {requestId=hex(p.validation.header.requestId),requestSHA256=hex(p.validation.header.requestSHA),totalBytes=p.validation.header.totalBytes,copiedBytes=math.min(p.validation.header.payloadBytes,p.validation.copiedCells*4),hashedBytes=p.validation.hashedBytes} or nil}
    end
    return api
end
ns.DuplexProtocol=API
