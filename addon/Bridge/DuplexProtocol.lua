local _, ns = ...

-- Duplex wire primitives and private single-request state machine. Public inbox
-- tables are transport only: all accepted data is copied into private strings.
local SHA = ns.SHA256
local M = 4294967296
local HEADER, FRAME_BYTES, CONTROL_BYTES = 320, 4096, 1024
local REQUEST_BYTES, FRAME_COUNT, FRAME_SLOTS = 1048576, 256, 1
local ZERO16, ZERO32 = string.rep("\0",16), string.rep("\0",32)
local MAGIC, SEND_MAGIC = "LYCMBX01", "LYCMSB01"
local KIND = {bind=1,frame=2,commit=3,cancel=4,close=5,resultAck=6,reload=7,lease=8,repair=9}
local KINDBYID = {[1]="bind",[2]="frame",[3]="commit",[4]="cancel",[5]="close",[6]="resultAck",[7]="reload",[8]="lease",[9]="repair"}
local LANES={"bindResume","close","cancel","commit","resultAck","reload","lease"}
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
    local out={}
    for i=first,first+count-1 do
        local v=rawget(words,i);if not numberCell(v) then return nil,"duplex_cell_invalid" end
        out[#out+1]=string.char(v%256,math.floor(v/256)%256,math.floor(v/65536)%256,math.floor(v/16777216)%256)
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
    local function cells(n)local a={};for i=1,n do a[i]=0 end;return a end
    local frames,control={},{}
    local roots={frameRows={},controlRows={}}
    for i=1,FRAME_SLOTS do local row=cells(80+FRAME_BYTES/4);frames[i]=row;roots.frameRows[i]=row end
    for _,lane in ipairs(LANES) do local row=cells(80+CONTROL_BYTES/4);control[lane]=row;roots.controlRows[lane]=row end
    local request={frames=frames};local calibration={0,1,4294967295,0.125,-13.5,7654321}
    roots.request=request;roots.frames=frames;roots.control=control;roots.calibration=calibration
    return {schema="lycheedev.mailbox.v1",generation=generation,calibration=calibration,request=request,control=control},roots
end

-- Converts 80 header words into bytes, rejecting secret and non-integer cells.
function API.ReadHeader(words)
    if not arrayShape(words,80+1024) then return fail("duplex_frame_shape") end
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
    local digest;digest,err=SHA.Digest("LYCMBX/frame/v1\0"..rawHeader:sub(1,232)..payload)
    if not digest then return nil,err end
    if digest~=h.frameSHA then return fail("duplex_frame_checksum") end
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
function API.Create(deps)
    if not safe(deps) or type(deps)~="table" or type(deps.identity)~="table" and type(deps.identity)~="function" or type(deps.publish)~="function" or type(deps.execute)~="function" or type(deps.compile)~="function" or type(deps.clock)~="function" or type(deps.encode)~="function" then return nil,"duplex_dependencies_invalid" end
    local private={identity=nil,owner=nil,fenceHi=0,fenceLo=0,request=nil,terminal=nil,closing=false,disabled=true,lanes={},controlPublication={},controlFingerprint={},statusSequence=0,publication=0,reloadPending=false,reload=nil,lastSeqHi=0,lastSeqLo=0,lastRequestRaw=nil,lastDigestRaw=nil}
    local protocol={}
    -- A negative fast path never grants execution. Weak keys cannot retain a
    -- retired arena; shadows contain only numbers, not Lua object addresses.
    local observedRows=setmetatable({}, {__mode="k"})
    local function pollCandidate(words,count)
        if not safe(words) or type(words)~="table" or getmetatable(words)~=nil then return true end
        if #words~=count or rawget(words,count+1)~=nil then return true end
        local lo,hi,tailLo,tailHi=rawget(words,75),rawget(words,76),rawget(words,77),rawget(words,78)
        if not numberCell(lo) or not numberCell(hi) or not numberCell(tailLo) or not numberCell(tailHi) then return true end
        if lo==0 and hi==0 or lo%2==1 or lo~=tailLo or hi~=tailHi then
            observedRows[words]=nil;return false
        end
        local old=observedRows[words]
        if old then
            local unchanged=true
            for i=1,count do
                local value=rawget(words,i)
                if not numberCell(value) or value~=old[i] then unchanged=false;break end
            end
            if unchanged then return false end
        end
        local shadow={}
        for i=1,count do
            local value=rawget(words,i)
            if not numberCell(value) then return true end
            shadow[i]=value
        end
        return true,shadow
    end
    local function rememberRow(words,shadow)
        if not shadow then return end
        for i=1,#shadow do
            local value=rawget(words,i)
            if not numberCell(value) or value~=shadow[i] then observedRows[words]=nil;return end
        end
        observedRows[words]=shadow
    end
    local function snapshotRecord(phase)
        private.statusSequence=private.statusSequence+1
        local i=private.identity or {}
        local r=private.request;local t=private.terminal
        local receipts={}
        for lane,v in pairs(private.lanes) do receipts[lane]={messageId=v.messageId,requestId=v.requestId,
            requestSHA256=v.requestSHA256 or string.rep("0",64),state=v.state,challenge=v.challenge} end
        local accepted={};if r then for n=1,r.nextFrame do accepted[n]=n end end
        local request=r and {requestId=r.requestId,requestSHA256=r.digest,requestSeq=uint64String(r.seqHi,r.seqLo),
            transportAttempt=uint64String(r.attemptHi,r.attemptLo),acceptedFrames=accepted,
            challenge=r.challenge or "",notStarted=r.state=="receiving" or r.state=="prepared"} or nil
        local terminal=t and t.resultSHA256 and {requestId=r and r.requestId or "",requestSHA256=r and r.digest or "",
            state=t.outcome,sha256=t.resultSHA256,bytes=t.resultBytes,pages=t.pages,pageSHA256=t.pageSHA256} or nil
        local actualPhase
        if private.quarantined or phase=="fault" then actualPhase="fault"
        elseif r and r.state=="terminal" then actualPhase=terminal and "result_pending" or "execution_unknown"
        elseif r then actualPhase=r.state
        elseif private.closing then actualPhase="closed"
        elseif private.released then actualPhase="released"
        else actualPhase="idle" end
        local actorOK=false
        if private.identity and deps.actor then
            local ok,actor=pcall(deps.actor)
            actorOK=ok and actor~=nil and actor.guid==private.identity.actorGUID and actor.character==private.identity.character and actor.realm==private.identity.realm or false
        end
        local businessReady=not private.disabled and not private.closing and private.owner~=nil and actorOK and (not r or r.state=="released")
        return {schema="lycheedev.mailbox.v1",layoutId="single-data-row-v1",runtimeToken=i.runtime or "",arenaGeneration=i.arenaGeneration or "",
            sessionToken=private.session or string.rep("0",32),ownerToken=private.owner or string.rep("0",32),
            actorBindingId=private.actorBinding or i.actorBindingId or string.rep("0",32),fence=uint64String(private.fenceHi,private.fenceLo),
            actorGUID=i.actorGUID or "",character=i.character or "",realm=i.realm or "",build=i.build or "",
            product=i.product or "",release=i.release or "",resourcesReleased=not t or t.resourcesReleased==true,
            phase=actualPhase,
            transportReady=not private.disabled and not private.closing,actorReady=actorOK,businessReady=businessReady,ready=businessReady,
            controlReady=not private.disabled,statusSequence=uint64String(0,private.statusSequence),heartbeat=uint64String(0,private.heartbeat or 1),
            request=request,receipts=receipts,terminal=terminal,
            repair=private.repair and {previousArena=private.repair.previousArena,newArena=private.repair.newArena,challenge=private.repair.challenge,
                requestId=private.repair.requestId,requestSHA256=private.repair.requestSHA256,notStarted=not private.repair.idle,ledgerRetained=true,
                previousWriterDrained=private.repair.previousWriterDrained==true,idle=private.repair.idle==true,
                noPendingRequest=private.repair.noPendingRequest==true,resourcesReleased=private.repair.resourcesReleased==true} or nil,
            released=private.released and {requestId=private.released.requestId,requestSHA256=private.released.requestSHA256} or nil}
    end
    local function publish(kind,record)
        private.publication=private.publication+1
        local wire,err=API.Sendbox(snapshotRecord(record and record.phase),65536);if not wire then return nil,err end
        local ok,reason=pcall(deps.publish,kind,wire)
        if not ok or reason==false then return nil,"duplex_sendbox_publish" end
        return true
    end
    local function identityMatches(h)
        local i=private.identity
        return i and h.runtime==tokenRaw(i.runtime,16) and h.arena==tokenRaw(i.arenaGeneration,16)
            and h.session==tokenRaw(private.session or string.rep("0",32),16)
            and h.owner==tokenRaw(private.owner or string.rep("0",32),16)
            and h.actorBinding==tokenRaw(private.actorBinding or string.rep("0",32),16)
            and h.fenceHi==private.fenceHi and h.fenceLo==private.fenceLo
    end
    local function receipt(lane,messageId,requestId,state,extra)
        local r={lane=lane,messageId=hex(messageId),requestId=requestId and hex(requestId) or string.rep("0",32),state=state}
        local context=private.receiptContext
        if context and context.messageId==r.messageId then r.requestSHA256=context.requestSHA256 end
        if extra then for k,v in pairs(extra) do r[k]=v end end
        if not r.requestSHA256 and private.request then r.requestSHA256=private.request.digest end
        private.lanes[lane]=r
        return publish("receipts",{phase=private.request and private.request.state or "idle",receipt=r})
    end
    local function terminalizeNotStarted(r,outcome,code)
        local body=deps.encode({ok=false,error=code,resourcesReleased=true},8192) or "{\"ok\":false,\"error\":\"cancelled_before_execution\",\"resourcesReleased\":true}"
        local sum=SHA.Digest(body);local pageSHA=SHA.Digest(body)
        r.state="terminal";r.fn=nil;r.source=nil;r.parts=nil;r.hash=nil;r.executionStarted=false
        private.terminal={outcome=outcome,failureCode=code,executionStarted=false,effects="none_started",resourcesReleased=true,resultSHA256=hex(sum),resultBytes=#body,pages=1,pageSHA256={hex(pageSHA)}}
        if deps.publishPage then pcall(deps.publishPage,1,body) end
        return publish("terminal",{phase="terminal",request={requestId=r.requestId,requestSHA256=r.digest},terminal=private.terminal})
    end
    local function newerSequence(hi,lo)
        return hi>private.lastSeqHi or hi==private.lastSeqHi and lo>private.lastSeqLo
    end
    local function readWords(lane,capacity)
        local words=lane
        if not arrayShape(words,80+capacity/4) then return nil,"duplex_lane_shape" end
        local hdr,err=wordsToBytes(words,1,80);if not hdr then return nil,err end
        local h;h,err=API.DecodeHeader(hdr);if not h then return nil,err end
        if h.payloadBytes%4~=0 or h.payloadBytes>capacity then return nil,"duplex_control_payload_length" end
        local payload;payload,err=wordsToBytes(words,81,h.payloadBytes/4)
        if not payload then return nil,err end
        local digest;digest,err=SHA.Digest("LYCMBX/frame/v1\0"..hdr:sub(1,232)..payload)
        if not digest or digest~=hdr:sub(233,264) then return nil,"duplex_control_checksum" end
        local again;again,err=wordsToBytes(words,1,80);if not again or again~=hdr then return nil,"duplex_control_changed" end
        for i=81+h.payloadBytes/4,80+capacity/4 do if rawget(words,i)~=0 then return nil,"duplex_control_padding" end end
        return h,payload
    end
    local function copyFrame(words)
        local hdr,err=API.ReadHeader(words);if not hdr then return nil,err end
        -- Snapshot the payload before checking the header and the publication stamp again.
        local h;h,err=API.DecodeHeader(hdr,KIND.frame);if not h then return nil,err end
        local payloadBytes=h.payloadBytes
        if payloadBytes>FRAME_BYTES then return nil,"duplex_frame_length" end
        local payload;payload,err=wordsToBytes(words,81,math.ceil(payloadBytes/4));if not payload then return nil,err end
        payload=payload:sub(1,payloadBytes)
        if payloadBytes%4~=0 then
            local last=rawget(words,80+math.ceil(payloadBytes/4))
            local used=payloadBytes%4
            if not numberCell(last) or math.floor(last/256^used)~=0 then return nil,"duplex_frame_padding" end
        end
        for i=81+math.ceil(payloadBytes/4),80+FRAME_BYTES/4 do if rawget(words,i)~=0 then return nil,"duplex_frame_padding" end end
        local headerAgain;headerAgain,err=API.ReadHeader(words);if not headerAgain then return nil,err end
        if headerAgain~=hdr then return nil,"duplex_frame_changed" end
        h,err=API.VerifyFrame(hdr,payload);if not h then return nil,err end
        return h,payload
    end
    local function processFrame(words)
        local h,payload=copyFrame(words);if not h then return nil,payload end
        if not identityMatches(h) then return nil,"duplex_frame_identity" end
        local r=private.request
        if not r then
            if h.frameIndex~=1 or private.closing or private.terminal or not newerSequence(h.requestSeqHi,h.requestSeqLo)
                or private.lastRequestRaw==h.requestId or private.lastDigestRaw==h.requestSHA then return nil,"duplex_request_unexpected" end
            local req={requestRaw=h.requestId,requestId=hex(h.requestId),seqHi=h.requestSeqHi,seqLo=h.requestSeqLo,digestRaw=h.requestSHA,digest=hex(h.requestSHA),attemptHi=h.attemptHi,attemptLo=h.attemptLo,totalBytes=h.totalBytes,frameCount=h.frameCount,budget=h.budget,utcHi=h.utcHi,utcLo=h.utcLo,state="receiving",parts={},bytes=0,nextFrame=0,hash=SHA.New()}
            local prefix="LYCMBX/request/v1\0"..h.requestId..h.actorBinding..u32le(h.budget)..u64word(h.utcHi,h.utcLo)..u32le(h.totalBytes)
            local hashOK,hashErr=SHA.Update(req.hash,prefix);if not hashOK then return nil,hashErr end
            if deps.clearPages then pcall(deps.clearPages) end
            private.request=req;r=req;private.lastSeqHi=h.requestSeqHi;private.lastSeqLo=h.requestSeqLo;private.lastRequestRaw=h.requestId;private.lastDigestRaw=h.requestSHA
        elseif h.requestId~=r.requestRaw or h.requestSeqHi~=r.seqHi or h.requestSeqLo~=r.seqLo or h.requestSHA~=r.digestRaw then
            return nil,"duplex_request_unexpected"
        end
        if r.state~="receiving" or h.frameIndex~=r.nextFrame+1 then return nil,"duplex_frame_order" end
        if h.frameCount~=r.frameCount or h.totalBytes~=r.totalBytes or h.budget~=r.budget or h.attemptHi~=r.attemptHi or h.attemptLo~=r.attemptLo then return nil,"duplex_frame_manifest_changed" end
        local expected=math.min(FRAME_BYTES,r.totalBytes-r.bytes)
        if expected<0 or #payload~=expected then return nil,"duplex_frame_extent" end
        local hashOK,hashErr=SHA.Update(r.hash,payload);if not hashOK then return nil,hashErr end
        r.parts[#r.parts+1]=payload;r.bytes=r.bytes+#payload;r.nextFrame=r.nextFrame+1
        private.released=nil;private.terminal=nil
            local ok,reason=publish("transfer",{phase="receiving",request={requestId=r.requestId,requestSHA256=r.digest,requestSeq=uint64String(r.seqHi,r.seqLo),acceptedFrames=r.nextFrame,frameCount=r.frameCount,receivedBytes=r.bytes,totalBytes=r.totalBytes}})
        if not ok then return nil,reason end
        if r.nextFrame<r.frameCount then return true end
        if r.bytes~=r.totalBytes then return nil,"duplex_request_extent" end
        local source=table.concat(r.parts);r.source=source
        local digest;digest,reason=SHA.Final(r.hash);r.hash=nil
        if not digest then return nil,reason end
        if digest~=r.digestRaw then return nil,"duplex_request_checksum" end
        local fn;fn,reason=deps.compile(source)
        r.parts=nil;r.source=nil
        if not fn then
            local body=deps.encode({ok=false,error="compile_error",resourcesReleased=true},8192) or "{\"ok\":false,\"error\":\"compile_error\",\"resourcesReleased\":true}"
            local sum=SHA.Digest(body);local pageSHA=SHA.Digest(body)
            r.state="terminal";r.executionStarted=false;r.source=nil;private.terminal={outcome="failed",failureCode="compile_error",executionStarted=false,effects="none_started",resourcesReleased=true,resultSHA256=hex(sum),resultBytes=#body,pages=1,pageSHA256={hex(pageSHA)}}
            if deps.publishPage then pcall(deps.publishPage,1,body) end
            return publish("terminal",{phase="terminal",request={requestId=r.requestId,requestSHA256=r.digest},terminal=private.terminal})
        end
        r.fn=fn;r.state="prepared";r.challengeRaw=deps.challenge and deps.challenge(r) or nil
        if type(r.challengeRaw)~="string" or #r.challengeRaw~=16 then return nil,"duplex_challenge_unavailable" end
        r.challenge=hex(r.challengeRaw)
        r.challengeAt=deps.clock()
        return publish("prepared",{phase="prepared",request={requestId=r.requestId,requestSHA256=r.digest,requestSeq=uint64String(r.seqHi,r.seqLo),challenge=r.challenge,transportAttempt=uint64String(r.attemptHi,r.attemptLo)}})
    end
    local function processControl(lane,words)
        local h,payload=readWords(words,CONTROL_BYTES);if not h then return nil,payload end
        if h.kind==KIND.frame then return nil,"duplex_control_kind" end
        if not identityMatches(h) and h.kind~=KIND.bind then return nil,"duplex_control_identity" end
        local laneName=h.laneName
        if lane~=laneName then return nil,"duplex_control_lane" end
        local expectedPayload=h.kind==KIND.resultAck and 92 or h.kind==KIND.lease and 16 or h.kind==KIND.bind and (#payload==88 and 88 or 0) or 0
        if #payload~=expectedPayload then return nil,"duplex_control_payload_length" end
        local old=private.lanes[laneName]
        local fingerprint=hex(SHA.Digest(h.raw..payload))
        if old and old.messageId==hex(h.messageId) then
            if private.controlFingerprint[laneName]~=fingerprint then return nil,"duplex_control_replay_changed" end
            return false
        end
        local prior=private.controlPublication[laneName]
        local ownerTransition=h.kind==KIND.bind and private.owner~=nil and private.owner~=hex(h.owner)
        if prior and not ownerTransition and (h.publicationSeqHi<prior.hi or h.publicationSeqHi==prior.hi and h.publicationSeqLo<=prior.lo) then return nil,"duplex_control_sequence" end
        if h.kind~=KIND.bind then
            private.controlPublication[laneName]={hi=h.publicationSeqHi,lo=h.publicationSeqLo}
            private.controlFingerprint[laneName]=fingerprint
        end
        private.receiptContext={messageId=hex(h.messageId),requestSHA256=hex(h.requestSHA)}
        if h.kind==KIND.bind then
            if h.runtime~=tokenRaw(private.identity.runtime,16) or h.arena~=tokenRaw(private.identity.arenaGeneration,16) then return nil,"duplex_bind_identity" end
            if h.requestId~=ZERO16 or h.requestSHA~=ZERO32 or h.requestSeqHi~=0 or h.requestSeqLo~=0 or h.budget~=0 or h.totalBytes~=0 then return nil,"duplex_bind_identity" end
            local expectedActor=tokenRaw(private.identity.actorBindingId or string.rep("0",32),16)
            local actor=deps.actor and deps.actor()
            if not expectedActor or h.actorBinding~=expectedActor or not actor or actor.guid~=private.identity.actorGUID then return nil,"duplex_bind_actor_mismatch" end
            if h.session==ZERO16 or h.owner==ZERO16 or h.actorBinding==ZERO16 or h.fenceHi==0 and h.fenceLo==0 then return nil,"duplex_bind_identity" end
            if private.owner and (private.owner~=hex(h.owner) or private.session~=hex(h.session)) then
                local oldOwner=tokenRaw(private.owner,16);local oldSession=tokenRaw(private.session,16)
                local nextFenceHi,nextFenceLo=private.fenceHi,private.fenceLo
                if nextFenceLo==4294967295 then nextFenceHi=nextFenceHi+1;nextFenceLo=0 else nextFenceLo=nextFenceLo+1 end
                local released=private.released
                local expected=(oldOwner or "")..(oldSession or "")..u64word(private.fenceHi,private.fenceLo)
                    ..(released and tokenRaw(released.requestId,16) or ZERO16)
                    ..(released and tokenRaw(released.requestSHA256,32) or ZERO32)
                if private.owner==hex(h.owner) or private.session==hex(h.session)
                    or #payload~=88 or not private.closing or private.request or private.terminal
                    or private.fenceHi==4294967295 and private.fenceLo==4294967295
                    or h.fenceHi~=nextFenceHi or h.fenceLo~=nextFenceLo or payload~=expected then
                    return nil,"duplex_bind_owner_conflict"
                end
                private.controlPublication={};private.controlFingerprint={};private.lanes={}
                private.lastSeqHi=0;private.lastSeqLo=0
            elseif #payload~=0 then
                return nil,"duplex_bind_owner_conflict"
            end
            if private.owner and not ownerTransition then
                if private.closing or private.actorBinding~=hex(h.actorBinding)
                    or private.fenceHi~=h.fenceHi or private.fenceLo~=h.fenceLo then
                    return nil,"duplex_bind_owner_conflict"
                end
            end
            -- Cached negative observations belong to one execution owner.
            -- Rebinding must reclassify even byte-identical old publications.
            observedRows=setmetatable({}, {__mode="k"})
            private.session=hex(h.session);private.owner=hex(h.owner);private.actorBinding=hex(h.actorBinding);private.fenceHi=h.fenceHi;private.fenceLo=h.fenceLo
            private.closing=false
            private.controlPublication[laneName]={hi=h.publicationSeqHi,lo=h.publicationSeqLo}
            private.controlFingerprint[laneName]=fingerprint
            return receipt(laneName,h.messageId,nil,"bound",{session=private.session,owner=private.owner,fence=uint64String(h.fenceHi,h.fenceLo),actorBindingId=private.actorBinding})
        end
        local r=private.request
        if h.kind==KIND.close then
            if r and (h.requestId~=r.requestRaw or h.requestSHA~=r.digestRaw) or not r and (h.requestId~=ZERO16 or h.requestSHA~=ZERO32) then return receipt(laneName,h.messageId,h.requestId,"rejected") end
            private.closing=true
            if r and (r.state=="receiving" or r.state=="prepared") then terminalizeNotStarted(r,"cancelled","closed_before_execution") end
            if r and r.state=="running" and r.execution then r.cancelRequested=true;pcall(function()return r.execution:RequestCancel()end) end
            local quiescent=not r or r.state=="released" or r.state=="terminal" and private.terminal and private.terminal.resourcesReleased==true
            return receipt(laneName,h.messageId,r and r.requestRaw,quiescent and "closed" or "closing",{quiescent=quiescent,businessReady=false})
        elseif h.kind==KIND.cancel then
            if not r then return receipt(laneName,h.messageId,h.requestId,"not_started",{executionStarted=false}) end
            if h.requestId~=r.requestRaw or h.requestSHA~=r.digestRaw then return receipt(laneName,h.messageId,h.requestId,"rejected") end
            if r.state=="receiving" or r.state=="prepared" then
                r.state="terminal";r.fn=nil;r.source=nil;r.parts=nil
                terminalizeNotStarted(r,"cancelled","cancelled_before_execution")
                return receipt(laneName,h.messageId,r.requestRaw,"cancelled",{executionStarted=false})
            elseif r.state=="running" then
                r.cancelRequested=true
                local ok,accepted=pcall(function()return r.execution:RequestCancel()end)
                if not ok then return receipt(laneName,h.messageId,r.requestRaw,"cancel_rejected",{failureCode="cancel_signal_failed"}) end
                if not accepted then return receipt(laneName,h.messageId,r.requestRaw,"cancel_pending",{failureCode="cancel_signal_pending"}) end
                return receipt(laneName,h.messageId,r.requestRaw,"cancel_requested",{executionStarted=true})
            end
            return receipt(laneName,h.messageId,r.requestRaw,"cancel_too_late",{executionStarted=r.executionStarted==true})
        elseif h.kind==KIND.commit then
            if not r or r.state~="prepared" or h.requestId~=r.requestRaw or h.requestSHA~=r.digestRaw or h.challenge~=r.challengeRaw then
                return receipt(laneName,h.messageId,h.requestId,"commit_rejected")
            end
            if private.closing then return receipt(laneName,h.messageId,r.requestRaw,"closing") end
            local actor=deps.actor and deps.actor()
            if not actor or not private.identity or actor.guid~=private.identity.actorGUID or actor.character~=private.identity.character or actor.realm~=private.identity.realm then return receipt(laneName,h.messageId,r.requestRaw,"actor_changed") end
            local now=deps.clock();if not safe(now) or type(now)~="number" or now~=now or now<0 or now==math.huge then return receipt(laneName,h.messageId,r.requestRaw,"clock_unavailable") end
            if not r.challengeAt or now<r.challengeAt or now-r.challengeAt>30 then return receipt(laneName,h.messageId,r.requestRaw,"challenge_expired") end
            r.state="running";r.executionStarted=true
            local started,failed
            if deps.started then pcall(deps.started,r.requestId) end
            local ok,handle=pcall(deps.execute,r.fn,math.min(120,math.max(1,math.ceil(r.budget/1000))),function(success,value,meta)
                if r.state~="running" then return end
                r.fn=nil;r.execution=nil
                local record={ok=success==true,resourcesReleased=not meta or meta.resourcesReleased~=false,logs=meta and meta.logs}
                if success==true then record.result=value else record.error=value end
                local json,err=deps.encode(record,524288)
                if not json then json=assert(deps.encode({ok=false,error="result_encoding_error",resourcesReleased=false},4096));success=false end
                local sum=SHA.Digest(json)
                local pages={};for at=1,#json,16384 do pages[#pages+1]=json:sub(at,at+16383) end
                if #pages==0 then pages[1]="" end
                local pageHashes={};for i,page in ipairs(pages) do pageHashes[i]=hex(SHA.Digest(page)) end
                private.terminal={outcome=meta and meta.cancelled==true and "cancelled" or success and "success" or "failed",executionStarted=true,failureCode=not success and (meta and meta.cancelled==true and "cancelled" or "probe_failed") or nil,effects="may_have_occurred",resourcesReleased=not meta or meta.resourcesReleased~=false,resultSHA256=hex(sum),resultBytes=#json,pages=#pages,pageSHA256=pageHashes}
                r.state="terminal"
                if deps.finished then pcall(deps.finished,r.requestId) end
                if deps.publishPage then for i,page in ipairs(pages) do
                    pcall(deps.publishPage,i,page)
                end end
                publish("terminal",{phase="terminal",request={requestId=r.requestId,requestSHA256=r.digest},terminal=private.terminal})
            end)
            if not ok then
                r.state="terminal";r.fn=nil;r.execution=nil
                private.terminal={outcome="failed",executionStarted=true,failureCode="executor_error",effects="may_have_occurred",resourcesReleased=false}
                if deps.finished then pcall(deps.finished,r.requestId) end
                return publish("terminal",{phase="terminal",request={requestId=r.requestId,requestSHA256=r.digest},terminal=private.terminal})
            end
            if r.state=="running" then r.execution=handle end
            return receipt(laneName,h.messageId,r.requestRaw,"accepted",{requestSHA256=r.digest})
        elseif h.kind==KIND.resultAck then
            if not r or r.state~="terminal" or h.requestId~=r.requestRaw or h.requestSHA~=r.digestRaw then return receipt(laneName,h.messageId,h.requestId,"ack_rejected") end
            -- Fixed 92-byte duplex.ResultAck: id, request digest, terminal enum,
            -- result digest, byte count and page count. No permissive JSON parser.
            if not API.VerifyResultAck(payload,r.requestId,r.digest,private.terminal) then return receipt(laneName,h.messageId,r.requestRaw,"ack_mismatch") end
            if not private.terminal.resourcesReleased then return receipt(laneName,h.messageId,r.requestRaw,"resources_pending") end
            private.released={requestId=r.requestId,requestSHA256=r.digest};r.state="released";r.fn=nil;r.execution=nil
            local permit=hex(SHA.Digest("LYCMBX/reuse/v1\0"..r.requestRaw..r.digestRaw..h.messageId))
            private.request=nil;private.terminal=nil
            if deps.clearPages then pcall(deps.clearPages) end
            return receipt(laneName,h.messageId,r.requestRaw,"released",{reusePermit=permit})
        elseif h.kind==KIND.reload then
            if not r and (h.requestId~=ZERO16 or h.requestSHA~=ZERO32) then return receipt(laneName,h.messageId,h.requestId,"rejected") end
            if r and (r.state~="released" or private.terminal and private.terminal.resourcesReleased~=true) then return receipt(laneName,h.messageId,r.requestRaw,"reload_blocked") end
            if type(deps.reload)~="function" then return receipt(laneName,h.messageId,h.requestId,"reload_unavailable") end
            local clockOK,now=pcall(deps.clock)
            local challengeOK,challengeRaw=pcall(function()return deps.challenge and deps.challenge(r)end)
            if not clockOK or not safe(now) or type(now)~="number" or now~=now or now<0 or now==math.huge
                or not challengeOK or type(challengeRaw)~="string" or #challengeRaw~=16 then return receipt(laneName,h.messageId,r and r.requestRaw or nil,"reload_unavailable") end
            private.reload={messageRaw=h.messageId,messageId=hex(h.messageId),challengeRaw=challengeRaw,challenge=hex(challengeRaw),expires=now+30}
            return receipt(laneName,h.messageId,r and r.requestRaw or nil,"accepted",{challenge=private.reload.challenge})
        elseif h.kind==KIND.lease then
            local reload=private.reload
            if reload then
                local clockOK,now=pcall(deps.clock)
                if not clockOK or not safe(now) or type(now)~="number" or now~=now or now<0 or now==math.huge then return receipt(laneName,h.messageId,h.requestId,"clock_unavailable") end
                if now>=reload.expires then private.reload=nil;return receipt(laneName,h.messageId,h.requestId,"reload_expired") end
                if h.requestId~=ZERO16 or h.requestSHA~=ZERO32 or h.challenge~=reload.challengeRaw or payload~=reload.messageRaw
                    or r and r.state~="released" or private.terminal and private.terminal.resourcesReleased~=true then
                    return receipt(laneName,h.messageId,h.requestId,"reload_lease_rejected")
                end
                private.reload=nil;private.reloadPending=true
                return receipt(laneName,h.messageId,h.requestId,"accepted")
            end
            return receipt(laneName,h.messageId,h.requestId,"lease_observed",{businessReady=not private.closing and (not r or r.state=="released")})
        elseif h.kind==KIND.repair then
            local repair=private.repair;local r=private.request
            if not repair or repair.idle and (r~=nil or private.terminal~=nil or h.requestId~=ZERO16 or h.requestSHA~=ZERO32
                or h.requestSeqHi~=0 or h.requestSeqLo~=0 or h.budget~=0 or h.totalBytes~=0)
                or not repair.idle and (not r or r.state~="receiving" and r.state~="prepared")
                or h.arena~=tokenRaw(repair.newArena,16) or h.challenge~=repair.challengeRaw
                or not repair.idle and (h.requestId~=r.requestRaw or h.requestSHA~=r.digestRaw or r.requestId~=repair.requestId
                or r.digest~=repair.requestSHA256) then return receipt(laneName,h.messageId,h.requestId,"repair_rejected") end
            if h.attemptHi==0 and h.attemptLo==0 then return receipt(laneName,h.messageId,h.requestId,"repair_rejected") end
            if not repair.idle then
                r.attemptHi=h.attemptHi;r.attemptLo=h.attemptLo;r.state="receiving";r.parts={};r.bytes=0;r.nextFrame=0
                r.hash=SHA.New()
                local prefix="LYCMBX/request/v1\0"..r.requestRaw..h.actorBinding..u32le(r.budget)..u64word(r.utcHi,r.utcLo)..u32le(r.totalBytes)
                local hashOK,hashErr=SHA.Update(r.hash,prefix);if not hashOK then return nil,hashErr end
                r.fn=nil;r.challenge=nil;r.challengeRaw=nil;r.challengeAt=nil;private.terminal=nil
            end
            repair.previousWriterDrained=true;private.repaired=true;private.lastFailure=nil
            return receipt(laneName,h.messageId,r and r.requestRaw or h.requestId,"repaired")
        end
        return receipt(laneName,h.messageId,h.requestId,"unsupported")
    end
    function protocol.BindIdentity(identity)
        if type(identity)~="table" or not tokenRaw(identity.runtime,16) or not tokenRaw(identity.arenaGeneration,16) then return nil,"duplex_identity_invalid" end
        if identity.actorBindingId and not tokenRaw(identity.actorBindingId,16) then return nil,"duplex_identity_invalid" end
        private.identity=detached(identity);return true
    end
    function protocol.PublishStatus()return publish("status",{phase="idle"})end
    function protocol.Heartbeat()
        private.heartbeat=(private.heartbeat or 1)+1
        return publish("status",{})
    end
    function protocol.ReleaseRetiredArena()
        if not private.repaired or not private.retiredArena then return nil,"duplex_retired_arena_unconfirmed" end
        private.retiredArena=nil;private.repair=nil;return true
    end
    function protocol.Quarantine(reason)
        private.disabled=true;private.quarantined=true;private.lastFailure=type(reason)=="string" and reason or "duplex_quarantined"
        return publish("status",{phase="fault"})
    end
    function protocol.BeginRepair(newArena,challengeRaw)
        local r=private.request
        local active=r and (r.state=="receiving" or r.state=="prepared")
        local idle=not r and not private.terminal and private.owner~=nil and private.session~=nil and not private.closing
        if private.repair or private.retiredArena or not active and not idle
            or type(newArena)~="string" or not tokenRaw(newArena,16) or newArena==private.identity.arenaGeneration
            or type(challengeRaw)~="string" or #challengeRaw~=16 then return nil,"duplex_repair_unavailable" end
        local previous=private.identity.arenaGeneration
        private.repair={previousArena=previous,newArena=newArena,challenge=hex(challengeRaw),challengeRaw=challengeRaw,
            requestId=r and r.requestId or string.rep("0",32),requestSHA256=r and r.digest or string.rep("0",64),
            previousWriterDrained=false,idle=idle,noPendingRequest=idle,resourcesReleased=idle and (not private.terminal or private.terminal.resourcesReleased==true)}
        private.retiredArena=true
        private.identity.arenaGeneration=newArena
        private.repaired=false;private.lastFailure=nil
        return true
    end
    function protocol.Poll(arena,budget)
        if private.disabled then return nil,"duplex_disabled" end
        budget=budget or 1
        if type(budget)~="number" or budget<1 or budget>8 or budget%1~=0 then return nil,"duplex_budget_invalid" end
        local processed=0
        for _,lane in ipairs(LANES) do
            if processed>=budget then break end
            local words=rawget(arena.control,lane)
            local candidate,shadow=pollCandidate(words,80+CONTROL_BYTES/4)
            if candidate then
                local h,payload=processControl(lane,words)
                rememberRow(words,shadow)
                if h then processed=processed+1 end
                if not h and payload and payload~="duplex_header_invalid" and payload~="duplex_publication_unstable" then private.lastFailure=payload end
            end
        end
        if processed<budget and not private.closing and (not private.request or private.request.state=="receiving") then
            local frames=arena.request.frames
            -- Logical frame numbers advance only after a private copy and ACK.
            -- All frames reuse one physical row; old/unknown writes cannot be
            -- overwritten merely because their host publication returned.
            local words=rawget(frames,1)
            local candidate,shadow=pollCandidate(words,80+FRAME_BYTES/4)
            if candidate then
                local ok,err=processFrame(words)
                rememberRow(words,shadow)
                if ok then processed=processed+1 elseif err and err~="duplex_publication_unstable" and err~="duplex_header_invalid" then private.lastFailure=err end
            end
        end
        if private.reload then
            local clockOK,now=pcall(deps.clock)
            if clockOK and safe(now) and type(now)=="number" and now==now and now>=private.reload.expires then private.reload=nil end
        end
        return processed
    end
    function protocol.TakeReload()
        if not private.reloadPending then return false end
        private.reloadPending=false
        return true
    end
    function protocol.Enable()if private.quarantined then return nil,"duplex_quarantined" end;private.disabled=false;return true end
    function protocol.RuntimeState()
        return private.quarantined==true,private.lastFailure,private.repaired==true
    end
    function protocol.Disable()
        local r=private.request
        if r and (r.state=="receiving" or r.state=="prepared") then
            terminalizeNotStarted(r,"cancelled","disabled_before_execution")
        elseif r and r.state=="running" then
            if not r.execution or type(r.execution.RequestCancel)~="function" then return nil,"duplex_cancel_unavailable" end
            local ok,accepted,reason=pcall(function()return r.execution:RequestCancel()end)
            if not ok or not accepted then return nil,reason or "duplex_cancel_pending" end
            if r.state=="running" then return nil,"duplex_cancel_pending" end
        end
        private.disabled=true
        return publish("status",{})
    end
    function protocol.Snapshot()
        local r=private.request
        local repair=private.repair and detached(private.repair) or nil
        if repair then repair.ledgerRetained=true;repair.notStarted=not repair.idle end
        return {identity=private.identity and detached(private.identity),phase=r and r.state or private.closing and "closed" or private.released and "released" or "idle",request=r and {requestId=r.requestId,requestSHA256=r.digest,requestSeq=uint64String(r.seqHi,r.seqLo),acceptedFrames=r.nextFrame,frameCount=r.frameCount,receivedBytes=r.bytes,totalBytes=r.totalBytes,challenge=r.challenge} or nil,terminal=private.terminal and detached(private.terminal),receipts=detached(private.lanes),closing=private.closing,disabled=private.disabled,quarantined=private.quarantined==true,lastFailure=private.lastFailure,released=private.released and detached(private.released),repair=repair,repaired=private.repaired==true,retiredArena=private.retiredArena==true,reloadPending=private.reloadPending,reload=private.reload and {messageId=private.reload.messageId,challenge=private.reload.challenge} or nil}
    end
    return protocol
end

ns.DuplexProtocol=API
