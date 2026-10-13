local _, ns = ...

-- v4 candidate. No frames/events/hooks/timers at load. Only private state authorizes work.
-- The historical v3 engine remains a frozen oracle until candidate cutover.
local function plain(v) return not (issecretvalue and issecretvalue(v)) and type(v)=="table" and getmetatable(v)==nil end
local function safe(v) return not (issecretvalue and issecretvalue(v)) end
local function integer(v,lo,hi) return safe(v) and type(v)=="number" and v==v and v%1==0 and v>=lo and v<=hi end
local function text(v,max) return safe(v) and type(v)=="string" and #v>0 and #v<=max end
local fields={"schema","index","runtime","owner","fence","sessionEpoch","requestSeq","digest","nonce","ticket","action","guid","build","challenge","code","budget","expires","resultBytes","resultChecksum","lane"}
local function copy(e)local out={} for _,k in ipairs(fields)do out[k]=e[k]end return out end
ns.NativeProtocol={Count=500,MaxInstances=8,Bootstrap=20,LaneSize=60,Create=function(a)
    assert(plain(a) and text(a.runtime,32) and type(a.encode)=="function" and type(a.clock)=="function","invalid native adapter")
    local wire=a.wire or ns.NativeWire
    local owner,fence,epoch,lane,cursor,active,closing=nil,nil,nil,nil,0,nil,false
    local consumed,requests,receipts,bodies={},{},{},{}
    local count,sequence,fused,lastRequest,controlBlocked,recoveryUsed=0,0,false,0,false,false
    local mailbox,publicReceipts,publicBodies,receivingControl
    local challenge=a.challenge()
    local function actor()
        local x=a.actor();if not plain(x) or not text(x.guid,128)then return end
        for _,key in ipairs({"character","realm"})do
            if not safe(x[key]) or (x[key]~=nil and (type(x[key])~="string" or #x[key]>256))then return end
        end
        return x
    end
    local function physical()if lane==nil then return nil end return 21+lane*60+cursor end
    local function public(value,kind,e)
        local payload,reason=a.encode(value,kind==3 and 524288 or 16384)
        if not payload then return nil,reason end
        sequence=sequence+1
        return wire.Encode(e and e.nonce or string.rep("0",32),a.runtime,e and e.ticket or string.rep("0",32),kind,1,sequence,payload)
    end
    local function receipt(e,state,extra)
        if state=="rejected" and receivingControl then fused=true;controlBlocked=true end
        local value={schema="lycheedev.slot.v4",runtime=a.runtime,owner=owner or "",fence=fence or 0,sessionEpoch=epoch or "",requestSeq=e.requestSeq or 0,
            digest=e.digest or "",nonce=e.nonce,ticket=e.ticket,state=state,action=e.action,nextSlot=physical(),challenge=challenge}
        for k,v in pairs(extra or {})do value[k]=v end
        local record,reason=public(value,2,e)
        if record then receipts[e.nonce]=record;if publicReceipts then rawset(publicReceipts,e.nonce,record)end end
        return record,reason
    end
    local api={}
    function api.AttachMailbox(value)
        assert(plain(value) and value.runtime==a.runtime and value.schema=="lycheedev.mailbox.v2","invalid native mailbox")
        mailbox=value;publicReceipts,publicBodies={},{};rawset(value,"receipts",publicReceipts);rawset(value,"bodies",publicBodies)
        for k,v in pairs(receipts)do rawset(publicReceipts,k,v)end
        for k,v in pairs(bodies)do rawset(publicBodies,k,v)end
    end
    function api.Identity()
        local x=actor();if not x then return nil,"native_actor_unavailable" end
        local record=public({schema="lycheedev.slot.identity.v3",runtime=a.runtime,owner=owner or "",fence=fence or 0,sessionEpoch=epoch or "",nextSlot=physical(),
            slots=500,inventory=a.inventory,lane=lane,challenge=challenge,guid=x.guid,character=x.character or "",realm=x.realm or "",product=a.product or "",build=a.build,release=a.release,fused=fused,closed=closing},1)
        if mailbox then rawset(mailbox,"identity",record)end
        return record
    end
    function api.NextSlot()return physical()end
    -- Detached telemetry, constructed from private state. Public mailbox edits
    -- never change the owner, route, challenge or execution gates.
    function api.InputIdentity()
        local x=actor();if not x then return nil end
        return {runtime=a.runtime,owner=owner or "",fence=fence or 0,sessionEpoch=epoch or "",nextSlot=physical() or 0,
            slots=500,inventory=a.inventory,lane=lane,challenge=challenge,guid=x.guid,character=x.character or "",realm=x.realm or "",product=a.product or "",build=a.build,release=a.release,fused=fused,closed=closing}
    end
    function api.Fused()return fused end
    function api.Route()return lane,cursor end
    function api.CanExecuteWake()return owner~=nil and not closing and active==nil and not fused end
    function api.CanSettle()return owner~=nil and not closing and not controlBlocked and active~=nil and active.state=="reported" end
    function api.CanClose()return owner~=nil and not closing and not controlBlocked and active==nil end
    local function observe(op,state,payload,ok)
        if not a.observe then return end
        local e=copy(op.envelope)
        e.codeBytes=op.codeBytes;e.codeChecksum=op.codeChecksum
        -- History consumes only a bounded preview and cannot mutate private
        -- request state or retain the full payload through this view.
        pcall(a.observe,{envelope=e,state=state,payload=payload,ok=ok})
    end
    local function activity(state,ticket)if a.activity then pcall(a.activity,state,ticket)end end
    local function finish(op,ok,value,metadata)
        if op.state~="running" then return nil,"native_not_running" end
        op.resourcesReleased=not metadata or metadata.resourcesReleased~=false
        local payload,reason=a.encode({ok=ok,result=ok and value or nil,error=not ok and value or nil,resourcesReleased=not metadata or metadata.resourcesReleased~=false,logs=metadata and metadata.logs},524288)
        if not payload then ok=false;payload=assert(a.encode({ok=false,error=reason or "native_encoding_failed"},16384)) end
        -- The immutable BODY already owns the full report. Keep only its length
        -- and checksum here; retaining payload would duplicate up to 512 KiB.
        op.bytes=#payload;op.checksum=wire.Checksum(payload)
        op.body=assert(wire.Encode(op.envelope.nonce,a.runtime,op.envelope.ticket,3,3,sequence,payload))
        bodies[op.envelope.ticket]=op.body;if publicBodies then rawset(publicBodies,op.envelope.ticket,op.body)end
        op.state="reported"
        observe(op,"reported",payload,ok);activity("collecting",op.envelope.ticket)
        return receipt(op.envelope,"reported",{reportBytes=op.bytes,reportChecksum=op.checksum})
    end
    local function exact(op,e)return op and op.envelope.digest==e.digest and op.envelope.ticket==e.ticket and op.envelope.sessionEpoch==e.sessionEpoch end
    function api.Receive(index,envelope,bootstrap,control)
        receivingControl=control==true
        if not integer(index,1,500) then return nil,"native_slot_invalid" end
        if consumed[index] then return nil,"native_slot_consumed" end
        if bootstrap then if index>20 then return nil,"native_bootstrap_route" end
        else
            if control and controlBlocked then return nil,"native_control_blocked"end
            if fused and not control then return nil,"native_route_fused" end
            if not lane or index~=physical() then return nil,"native_foreign_route" end
            -- Stop on first empty/malformed load. Late wakes cannot eat the remaining route.
            cursor=cursor+1
        end
        consumed[index]=true
        if not plain(envelope) then if not bootstrap then fused=true;controlBlocked=control end return nil,"native_empty_or_invalid" end
        for _,k in ipairs(fields)do if not safe(envelope[k]) then if not bootstrap then fused=true;controlBlocked=control end return nil,"native_secret_envelope" end end
        local e=copy(envelope)
        if e.schema~="lycheedev.slot.v4" or e.index~=index or not wire.Token(e.nonce) or not wire.Token(e.ticket) or not wire.Token(e.owner)
            or e.runtime~=a.runtime or e.nonce==string.rep("0",32) or not integer(e.fence,1,9007199254740991) then if not bootstrap then fused=true;controlBlocked=control end return nil,"native_envelope_invalid" end
        if receipts[e.nonce]then if not bootstrap then fused=true;controlBlocked=control end return nil,"native_nonce_reused"end
        local x=actor();if not x or e.guid~=x.guid or e.build~=a.build then return receipt(e,"rejected",{reason="native_target_changed"}) end
        if control and e.action~="settle" and e.action~="close" then fused=true;controlBlocked=true;return receipt(e,"rejected",{reason="native_control_action"})end
        if e.action=="open" then
            if not bootstrap or a.inventory~=500 or owner or not integer(e.lane,0,7) or index~=e.lane*2+1 or not wire.Token(e.sessionEpoch) or e.challenge~=challenge then return receipt(e,"rejected",{reason="native_open_invalid"})end
            if epoch and (epoch==e.sessionEpoch or e.fence<=fence) then return receipt(e,"rejected",{reason="native_reopen_fence"})end
            -- Reopen preserves the consumed route cursor. A new epoch cannot recycle LoD.
            if lane and lane~=e.lane then return receipt(e,"rejected",{reason="native_route_changed"})end
            owner,fence,epoch,lane=e.owner,e.fence,e.sessionEpoch,e.lane
            closing=false;lastRequest=0;requests={}
            challenge=a.challenge();api.Identity();return receipt(e,"opened")
        end
        if owner~=e.owner or fence~=e.fence or epoch~=e.sessionEpoch or closing then return receipt(e,"rejected",{reason="native_owner_mismatch"}) end
        if e.action=="recover" then
            if not bootstrap or index~=lane*2+2 or not fused or e.challenge~=challenge then return receipt(e,"rejected",{reason="native_recovery_invalid"}) end
            fused=false;controlBlocked=false;recoveryUsed=true;challenge=a.challenge();api.Identity();return receipt(e,"recovered")
        end
        if bootstrap then return receipt(e,"rejected",{reason="native_control_route"}) end
        if e.action~="execute" and not control then return receipt(e,"rejected",{reason="native_control_required"})end
        if e.action=="execute" then
            if not integer(e.requestSeq,1,9007199254740991) or not text(e.digest,64) then return receipt(e,"rejected",{reason="native_request_invalid"})end
            local prior=requests[e.requestSeq]
            if prior then
                if not exact(prior,e) or (prior.code and prior.code~=e.code) then return receipt(e,"rejected",{reason="native_request_conflict"}) end
                if prior.state=="reported" then return receipt(e,"reported",{reportBytes=prior.bytes,reportChecksum=prior.checksum})end
                return receipt(e,prior.state)
            end
            local now=a.clock()
            if active or recoveryUsed or count>=28 or cursor+3>=60 or e.requestSeq~=lastRequest+1 or e.challenge~=challenge or not text(e.code,262144) or e.code:byte(1)==27
                or not integer(e.budget,1,120) or not integer(e.expires,1,4294967295) or not safe(now) or type(now)~="number" or now~=now
                or now<0 or now>=4294967295 or now>e.expires then return receipt(e,"rejected",{reason="native_execute_invalid"}) end
            local compiled,fn,reason=pcall(a.compile,e.code)
            local op={envelope=e,code=e.code,codeBytes=#e.code,codeChecksum=wire.Checksum(e.code),state="running",deadline=now+e.budget}
            -- Private request registration precedes every possible business side effect.
            requests[e.requestSeq]=op;active=op;count=count+1;lastRequest=e.requestSeq;challenge=a.challenge()
            observe(op,"running");activity("running",e.ticket)
            if a.releaseInput and not pcall(a.releaseInput)then return finish(op,false,"native_input_release_failed",{resourcesReleased=false})end
            receipt(e,"running")
            if not compiled or not fn then
                local message=compiled and reason or "native_compile_failed"
                if not safe(message) or type(message)~="string"then message="native_compile_failed"end
                return finish(op,false,{kind="compile_error",message=message:sub(1,4096)})
            end
            local called,ok,value,metadata=pcall(a.execute,fn,e.budget,function(ok,value,metadata)return finish(op,ok,value,metadata)end)
            if not called then finish(op,false,"native_executor_failed",{resourcesReleased=false})elseif op.state=="running" and ok~=nil then finish(op,ok,value,metadata)end
            api.Identity();return receipts[e.nonce]
        end
        if e.action=="settle" then
            local op=requests[e.requestSeq]
            if not exact(op,e) then return receipt(e,"rejected",{reason="native_settle_identity"})end
            if op.state=="settled" then return receipt(e,"settled",{resourcesReleased=op.resourcesReleased,displayCleared=op.displayCleared})end
            if op.state~="reported" or op.bytes~=e.resultBytes or op.checksum~=e.resultChecksum then return receipt(e,"rejected",{reason="native_settle_result"})end
            local cleared=true
            if a.cleanup then local ok,result=pcall(a.cleanup,e.ticket);cleared=ok and result==true end
            op.displayCleared=cleared
            observe(op,"released");activity("idle",e.ticket)
            -- Keep only the small terminal identity; never evict unacknowledged results.
            op.state="settled";op.body=nil;op.payload=nil;op.code=nil;op.envelope.code=nil
            bodies[e.ticket]=nil;if publicBodies then rawset(publicBodies,e.ticket,nil)end
            active=nil;fused=false;controlBlocked=false;api.Identity();return receipt(e,"settled",{resourcesReleased=op.resourcesReleased,displayCleared=cleared})
        end
        if e.action=="close" then
            if active then return receipt(e,"rejected",{reason="native_pending_requires_settle"})end
            closing=true;owner=nil;api.Identity();return receipt(e,"closed")
        end
        return receipt(e,"rejected",{reason="native_action_invalid"})
    end
    return api
end}
