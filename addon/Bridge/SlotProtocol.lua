local _, ns = ...

-- Pure bounded protocol engine. Host input, frames, file loading and async
-- execution are adapters; there is only one owner and one active business job.
local ZERO=string.rep("0",32)
local LIMIT=200
local MAX_OPERATIONS=math.floor((LIMIT-16)/4)
local function plain(v)return not (issecretvalue and issecretvalue(v)) and type(v)=="table" and getmetatable(v)==nil end
local function safe(v)return not (issecretvalue and issecretvalue(v))end
local envelopeFields={"schema","index","runtime","owner","fence","nonce","ticket","action","guid","build",
    "challenge","preparedNonce","reportBytes","reportChecksum","codeBytes","codeChecksum","budget"}
local function copyEnvelope(e,withCode)
    local copy={}
    for _,key in ipairs(envelopeFields) do
        local value=e[key]
        if safe(value) and (type(value)=="string" or type(value)=="number" or type(value)=="boolean") then copy[key]=value end
    end
    if withCode then copy.code=e.code end
    return copy
end

ns.SlotProtocol = { Count=LIMIT, Create=function(adapter)
    assert(plain(adapter) and ns.MemoryProtocol.Token(adapter.runtime),"invalid slot adapter")
    local wire=ns.MemoryProtocol
    local owner,fence,active,pending
    local sequence,count,cursor=0,0,1
    local consumed,operations,receipts={},{},{}
    local descriptor
    -- Passive copies only. Public maps must never alias protocol state: callers
    -- may inspect or overwrite them without changing nonce/owner decisions.
    local mailbox,mailboxReceipts,mailboxBodies
    local api={}
    function api.AttachMailbox(publication)
        assert(plain(publication) and publication.schema=="lycheedev.mailbox.v1"
            and publication.runtime==adapter.runtime and publication.release==adapter.release,"invalid slot mailbox")
        mailbox,mailboxReceipts,mailboxBodies=publication,{},{}
        rawset(mailbox,"receipts",mailboxReceipts);rawset(mailbox,"bodies",mailboxBodies)
        rawset(mailbox,"identity",descriptor)
        for nonce,record in pairs(receipts) do rawset(mailboxReceipts,nonce,record) end
        for ticket,op in pairs(operations) do if op.body then rawset(mailboxBodies,ticket,op.body) end end
    end
    local function observe(op)
        if adapter.observe then
            local ok,result,reason=pcall(adapter.observe,op)
            op.historyError=not ok and "automation_history_failed" or (not result and reason or nil)
        end
    end
    local function nextSlot()
        while cursor<=LIMIT and consumed[cursor] do cursor=cursor+1 end
        return cursor
    end
    local function actor()
        local value=adapter.actor()
        if not plain(value) or not safe(value.guid) or not safe(value.character) or not safe(value.realm)
            or type(value.guid)~="string" or type(value.character)~="string" or type(value.realm)~="string" then return nil end
        return value
    end
    local function encode(value,limit)
        local text,reason=adapter.encode(value,limit)
        if not text then return nil,reason end
        return text
    end
    local function publish(e,state,extra,kind)
        if sequence>=4294967295 then return nil,"slot_sequence_exhausted" end
        local a=actor();if not a then return nil,"slot_actor_unavailable" end
        sequence=sequence+1
        local value={schema="lycheedev.slot.v3",runtime=adapter.runtime,owner=owner or "",fence=fence or 0,
            nonce=e.nonce,ticket=e.ticket or ZERO,action=e.action,state=state,sequence=sequence,
            nextSlot=nextSlot(),slots=LIMIT,character=a.character,realm=a.realm,guid=a.guid,
            build=adapter.build,product=adapter.product,release=adapter.release,inventory=adapter.inventory,inputState=adapter.inputState}
        for k,v in pairs(extra or {}) do value[k]=v end
        local text,reason=encode(value,16384);if not text then return nil,reason end
        local record;record,reason=wire.Encode(e.nonce,adapter.runtime,e.ticket or ZERO,kind or 2,1,sequence,text)
        if not record then return nil,reason end
        receipts[e.nonce]=record
        if mailboxReceipts then rawset(mailboxReceipts,e.nonce,record) end
        return record,value
    end
    function api.Describe()
        local a=actor();if not a then return nil,"slot_actor_unavailable" end
        local text=encode({schema="lycheedev.slot.identity.v2",runtime=adapter.runtime,owner=owner or "",fence=fence or 0,
            nextSlot=nextSlot(),slots=LIMIT,character=a.character,realm=a.realm,guid=a.guid,
            build=adapter.build,product=adapter.product,release=adapter.release,inventory=adapter.inventory,inputState=adapter.inputState},16384)
        if not text then return nil,"slot_identity_encoding" end
        descriptor=wire.Encode(ZERO,adapter.runtime,ZERO,1,1,sequence,text)
        if mailbox then rawset(mailbox,"identity",descriptor) end
        return descriptor
    end
    local function finish(op,ok,value,metadata)
        if op.state~="running" then return nil,"slot_not_running" end
        op.resourcesReleased=not metadata or metadata.resourcesReleased~=false
        local text,reason=encode({ok=ok,result=ok and value or nil,error=not ok and value or nil,
            resourcesReleased=op.resourcesReleased,logs=metadata and metadata.logs or nil},524288)
        if not text then ok=false;text=assert(encode({ok=false,error=type(reason)=="string" and reason or "report_encoding_failed"},16384)) end
        -- Store body and outcome before constructing its public response.
        op.payload=text;op.bytes=#text;op.checksum=wire.Checksum(text)
        op.body=assert(wire.Encode(op.envelope.nonce,adapter.runtime,op.envelope.ticket,3,3,op.sequence,text))
        if mailboxBodies then rawset(mailboxBodies,op.envelope.ticket,op.body) end
        op.state="reported";op.ok=ok
        observe(op)
        local record,value=publish(op.envelope,"reported",{reportBytes=op.bytes,reportChecksum=op.checksum,challenge=op.challenge})
        if adapter.activity then adapter.activity("collecting",op.envelope.ticket) end
        return record,value
    end
    local function reject(e,reason)
        if plain(e) and wire.Token(e.nonce) and (e.ticket==nil or wire.Token(e.ticket)) then return publish(e,"rejected",{reason=reason}) end
        return nil,reason
    end
    function api.Receive(index,e)
        if type(index)~="number" or index%1~=0 or index<1 or index>LIMIT then return nil,"slot_index_invalid" end
        if consumed[index] then return nil,"slot_already_consumed" end
        consumed[index]=true -- Load-on-demand is consumed even by invalid data.
        if e==nil then return nil,"slot_skipped_empty",true end
        if not plain(e) then return nil,"slot_envelope_invalid" end
        -- Never inspect or execute encoded business code before these checks.
        for _,key in ipairs({"schema","index","runtime","owner","fence","nonce","ticket","action","guid","build","challenge","preparedNonce","reportBytes","reportChecksum"}) do if not safe(e[key]) then return nil,"slot_secret_envelope" end end
        if e.schema~="lycheedev.slot.v3" or e.index~=index or not wire.Token(e.nonce)
            or not wire.Token(e.ticket) or not wire.Token(e.owner) or not wire.Token(e.runtime) then return nil,"slot_envelope_invalid" end
        if e.runtime~=adapter.runtime then
            if type(e.guid)~="string" or type(e.build)~="string"
                or type(e.fence)~="number" or e.fence<1 or e.fence%1~=0 or e.fence>9007199254740991
                or (e.action~="bind" and e.action~="prepare" and e.action~="commit" and e.action~="confirm" and e.action~="release" and e.action~="unbind") then return nil,"slot_envelope_invalid" end
            return nil,"slot_skipped_foreign",true
        end
        if receipts[e.nonce] then return receipts[e.nonce],"slot_nonce_reused" end
        local a=actor();if not a or a.guid~=e.guid or e.build~=adapter.build then return reject(e,"slot_target_changed") end
        if type(e.fence)~="number" or e.fence<1 or e.fence%1~=0 or e.fence>9007199254740991 then return reject(e,"slot_fence_invalid") end
        if e.action=="bind" then
            if owner and owner~=e.owner then return reject(e,"slot_owner_busy") end
            if fence and e.fence<fence then return reject(e,"slot_stale_driver") end
            owner,fence=e.owner,e.fence
            api.Describe()
            return publish(e,"bound")
        end
        if owner~=e.owner or fence~=e.fence then return reject(e,"slot_owner_mismatch") end
        if e.action=="prepare" then
            if active or pending then return reject(e,"slot_busy") end
            if operations[e.ticket] then return reject(e,"slot_operation_exists") end
            if nextSlot()>LIMIT-3 or count>=MAX_OPERATIONS then return reject(e,"slot_capacity") end
            if not safe(e.code) or type(e.code)~="string" or #e.code<1 or #e.code>262144 or e.code:byte(1)==27
                or not safe(e.codeBytes) or e.codeBytes~=#e.code or not safe(e.codeChecksum) or e.codeChecksum~=wire.Checksum(e.code)
                or not safe(e.budget) or type(e.budget)~="number" or e.budget%1~=0 or e.budget<1 or e.budget>120 then return reject(e,"slot_code_invalid") end
            -- The loading addon owns e. Retain only checked immutable values,
            -- never its mutable table or unknown fields, before adapter calls.
            e=copyEnvelope(e,true)
            local fn,reason=adapter.compile(e.code)
            local challenge=adapter.runtime:sub(1,24)..string.format("%08x",sequence+1)
            if not fn then
                local op={envelope=e,challenge=challenge,sequence=sequence+1,state="running"}
                operations[e.ticket]=op;active=op;count=count+1
                if adapter.releaseInput then adapter.releaseInput() end
                return finish(op,false,{kind="compile_error",message=reason or "slot_compile_failed"})
            end
            pending={envelope=e,fn=fn,challenge=challenge,sequence=sequence+1,state="prepared"}
            operations[e.ticket]=pending;count=count+1
            observe(pending)
            return publish(e,"prepared",{challenge=challenge})
        elseif e.action=="commit" then
            if not pending or pending.envelope.ticket~=e.ticket or pending.challenge~=e.challenge
                or pending.envelope.nonce~=e.preparedNonce then return reject(e,"slot_commit_mismatch") end
            active=pending;pending=nil;active.state="running"
            local op=active
            observe(op)
            local record,value=publish(e,"accepted")
            if adapter.releaseInput then adapter.releaseInput() end
            if adapter.activity then adapter.activity("running",e.ticket) end
            local ok,reason=pcall(adapter.execute,op.fn,op.envelope.budget,function(ok,result,metadata)return finish(op,ok,result,metadata)end)
            if not ok and op.state=="running" then finish(op,false,"probe_executor_error") end
            return record,value
        elseif e.action=="confirm" then
            local op=operations[e.ticket]
            if not op then return publish(e,"absent",nil,4) end
            return publish(e,op.state,{preparedNonce=op.envelope.nonce,reportBytes=op.bytes,reportChecksum=op.checksum,challenge=op.challenge},4)
        elseif e.action=="release" then
            local op=operations[e.ticket]
            if not op or (op.state~="reported" and op.state~="released")
                or e.reportBytes~=op.bytes or e.reportChecksum~=op.checksum then return reject(e,"slot_release_mismatch") end
            if op.resourcesReleased==false then return reject(e,"slot_resources_pending") end
            op.payload=nil;op.body=nil;op.fn=nil;op.state="released"
            if mailboxBodies then rawset(mailboxBodies,e.ticket,nil) end
            -- Retain identity/digest tombstone; never re-admit its ticket.
            op.envelope.code=nil
            observe(op)
            if active==op then active=nil end
            if adapter.activity then adapter.activity("idle",e.ticket) end
            return publish(e,"released")
        elseif e.action=="unbind" then
            if active or pending then return reject(e,"slot_busy") end
            local record,value=publish(e,"unbound")
            owner,fence=nil,nil;api.Describe();return record,value
        end
        return reject(e,"slot_action_invalid")
    end
    function api.NextSlot()return nextSlot()end
    function api.InputIdentity()
        local a=actor();if not a then return nil end
        return {runtime=adapter.runtime,owner=owner or "",fence=fence or 0,nextSlot=nextSlot(),
            guid=a.guid,build=adapter.build,character=a.character,realm=a.realm}
    end
    function api.Snapshot()
        -- At most 200 receipts/consumed bits and 46 operation fact objects.
        -- Immutable strings may be shared; execution functions, callbacks,
        -- code and writable private tables never leave the engine.
        local result={descriptor=descriptor,receipts={},operations={},owner=owner,fence=fence,consumed={}}
        for nonce,record in pairs(receipts) do result.receipts[nonce]=record end
        for i=1,LIMIT do if consumed[i] then result.consumed[i]=true end end
        for ticket,op in pairs(operations) do
            result.operations[ticket]={envelope=copyEnvelope(op.envelope,false),challenge=op.challenge,
                sequence=op.sequence,state=op.state,body=op.body,bytes=op.bytes,checksum=op.checksum,
                ok=op.ok,resourcesReleased=op.resourcesReleased}
        end
        return result
    end
    if adapter.mailbox then api.AttachMailbox(adapter.mailbox) end
    return api
end }
