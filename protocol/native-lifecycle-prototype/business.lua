-- Shared private business receiver for transport comparisons. Never imported
-- by addon/product files. Transport must authenticate an exact request before
-- dispatch(); this module does not accept public Mailbox booleans as authority.
local M={}
local MAX_BODY,MAX_CODE,MAX_HISTORY=524288,262144,16
local MAX_CLEANUP_ATTEMPTS,CLEANUP_MILLIS=4,5000
local function hex(s,n) return type(s)=='string' and #s==n and not s:find('[^0-9a-f]') end
function M.new(a)
    assert(type(a.clock)=='function' and type(a.sha256)=='function' and type(a.execute)=='function')
    assert(type(a.inspect_resources)=='function' and type(a.close_resources)=='function')
    assert(type(a.publish_body)=='function' and type(a.clear_body)=='function' and type(a.publish_state)=='function')
    local task,completion,lastCommand=nil,nil,nil
    local closed=false
    local history,order={},{}
    local highWater,revision,executionStarts=0,0,0
    local highestClock=nil
    local function now()
        local ok,t=pcall(a.clock)
        if not ok or type(t)~='number' or t<0 or t%1~=0 or t>4294967295 then return nil end
        if highestClock and t<highestClock then return nil end
        highestClock=t
        return t -- Clock truth/cross-pause semantics remain a P1 obligation.
    end
    local function snapshot()
        return {
            revision=revision,highWater=highWater,executionStarts=executionStarts,closed=closed,
            commandNumber=lastCommand and lastCommand.number or 0,
            commandDigest=lastCommand and lastCommand.digest or '',
            commandOutcome=lastCommand and lastCommand.outcome or 'none',
            commandTask=lastCommand and lastCommand.task or 0,
            commandResultRef=lastCommand and lastCommand.resultRef or '',
            task=task and task.id or 0,taskDigest=task and task.digest or '',
            phase=task and task.phase or 'none',resultState=task and task.resultState or 'none',
            resultRef=task and task.resultRef or '',bodyPresent=task and task.body~=nil or false,
            bodyBytes=task and task.body and #task.body or 0,
            executionDeadline=task and task.executionDeadline or 0,
            cleanup=task and task.cleanup or 'none',cleanupAttempts=task and task.cleanupAttempts or 0,
            bodyReleaseAttempts=task and task.bodyReleaseAttempts or 0,
            cleanupDeadline=task and task.cleanupDeadline or 0,
            completionTask=completion and completion.task or 0,
            completionResultRef=completion and completion.resultRef or '',
            completionReceivedNumber=completion and completion.number or 0,
            completionReceivedDigest=completion and completion.digest or '',
            completionResultState=completion and completion.resultState or 'none',
        } -- Detached flat scalar copy: publication cannot mutate private task.
    end
    local function emit()
        revision=revision+1
        -- Actual BODY publication precedes the state that makes it visible.
        -- Faults leave private bytes intact; republish never executes user code.
        if task and task.body and a.publish_body(task.id,task.resultRef,task.body)~=true then error('body_publication_unconfirmed') end
        if a.publish_state(snapshot())~=true then error('state_publication_unconfirmed') end
    end
    local function safe_emit()
        local ok,why=pcall(emit);return ok,ok and nil or tostring(why)
    end
    local function remember(c,outcome,resultRef)
        local entry={number=c.number,digest=c.digest,task=c.task,outcome=outcome,resultRef=resultRef or ''}
        highWater=c.number;history[c.number]=entry;order[#order+1]=c.number;lastCommand=entry
        if #order>MAX_HISTORY then history[table.remove(order,1)]=nil end
        return entry
    end
    local function response(c,outcome,resultRef)
        local entry=remember(c,outcome,resultRef);local published,error=safe_emit()
        return {outcome=entry.outcome,number=entry.number,digest=entry.digest,published=published,publicationError=error}
    end
    local function finish(id,resultState,body)
        if not task or task.id~=id or (task.phase~='running' and task.phase~='running_unknown') then return false,false,'late_completion_ignored' end
        if (resultState~='verified' and resultState~='unavailable') or type(body)~='string' or #body>MAX_BODY then
            resultState,body='unavailable','result_encoding_or_limit'
        end
        local digest=a.sha256(body);assert(hex(digest,64),'body_digest')
        task.phase,task.resultState,task.body,task.resultRef='terminal',resultState,body,digest
        task.code=nil
        task.cleanup='pending'
        local ok,why=safe_emit();return true,ok,why
    end
    local function inspect(t)
        -- The adapter must inspect its OWN resource owner and real state. A
        -- callback return/exception does not prove either effect or cleanup.
        local ok,x=pcall(a.inspect_resources,t.owner)
        if not ok or type(x)~='table' or x.owner~=t.owner or type(x.closed)~='boolean' then return nil end
        return x.closed
    end
    local function received(c)
        if not task or task.id~=c.task or task.digest~=c.taskDigest then return response(c,'task_identity_mismatch') end
        if task.phase~='terminal' or not task.body then return response(c,'result_not_ready') end
        if c.resultRef~=task.resultRef then return response(c,'result_identity_mismatch',task.resultRef) end
        local t=task
        if t.cleanupDeadline==0 then local time=now();if not time or time>4294962295 then return response(c,'clock_unavailable',t.resultRef) end;t.cleanupDeadline=time+CLEANUP_MILLIS end
        local closed=inspect(t)
        if closed==nil then t.cleanup='unknown';return response(c,'cleanup_unknown',t.resultRef) end
        if not closed then
            local time=now()
            if not time or time>t.cleanupDeadline or t.cleanupAttempts>=MAX_CLEANUP_ATTEMPTS then
                t.cleanup='blocked';return response(c,'cleanup_budget_exhausted',t.resultRef)
            end
            t.cleanupAttempts=t.cleanupAttempts+1
            local ok,confirmed=pcall(a.close_resources,t.owner)
            if not ok or confirmed~=true then
                -- Even an effect followed by exception keeps BODY. A later
                -- exact new-number retry inspects first and does not blindly
                -- repeat an opaque side effect that may already have happened.
                t.cleanup='unknown';return response(c,'cleanup_unknown',t.resultRef)
            end
            closed=inspect(t)
            if closed~=true then t.cleanup=closed==nil and 'unknown' or 'pending';return response(c,'cleanup_pending',t.resultRef) end
        end
        t.cleanup='verified'
        local releaseTime=now()
        if not releaseTime or releaseTime>t.cleanupDeadline or t.bodyReleaseAttempts>=MAX_CLEANUP_ATTEMPTS then
            t.cleanup='release_blocked';return response(c,'cleanup_budget_exhausted',t.resultRef)
        end
        t.bodyReleaseAttempts=t.bodyReleaseAttempts+1
        local cleared,confirmed=pcall(a.clear_body,t.id,t.resultRef)
        if not cleared or confirmed~=true then t.cleanup='release_pending';return response(c,'body_release_pending',t.resultRef) end
        -- Only verified resource cleanup + successful exact public release can
        -- release private bytes. Completion remains separately bound to task
        -- and result even when unrelated commands overwrite lastCommand.
        completion={task=t.id,resultRef=t.resultRef,resultState=t.resultState,number=c.number,digest=c.digest}
        t.body=nil;task=nil
        return response(c,'cleanup_complete',completion.resultRef)
    end
    local function run(c)
        if task then return response(c,'task_busy') end
        if c.task~=c.number or c.taskDigest~=c.digest or not hex(c.taskDigest,64) or type(c.code)~='string' or #c.code>MAX_CODE
            or type(c.budgetMillis)~='number' or c.budgetMillis%1~=0 or c.budgetMillis<1000 or c.budgetMillis>120000 then return response(c,'run_fields') end
        local time=now();if not time or time+c.budgetMillis>4294962295 then return response(c,'clock_unavailable') end
        -- Budget and execution ownership are assigned exactly once. Resume or
        -- result publication retry never creates a new execution deadline.
        local t={id=c.task,digest=c.taskDigest,code=c.code,owner=string.format('%08x',c.task)..':'..c.taskDigest,
            phase='running',resultState='none',body=nil,resultRef='',executionDeadline=time+c.budgetMillis,
            cleanup='pending',cleanupAttempts=0,bodyReleaseAttempts=0,cleanupDeadline=0,cancelRequested=false,timeoutStopAttempted=false}
        task=t;executionStarts=executionStarts+1
        local entry=remember(c,'accepted')
        local finishPublication=nil
        local started,confirmed=pcall(a.execute,t.owner,c.code,c.budgetMillis,function(resultState,body)
            local accepted,published,error=finish(t.id,resultState,body)
            finishPublication={accepted=accepted,published=published,error=error}
            return accepted,published,error
        end)
        if (not started or confirmed~=true) and task==t and t.phase=='running' then
            -- The producer may still complete after a launch exception. Keep
            -- ownership and no BODY until its exact completion or verified
            -- stop; do not publish a false terminal business outcome.
            t.phase,t.resultState='running_unknown','unavailable'
            entry.outcome='accepted_execution_unknown'
        end
        local published,why
        if finishPublication and finishPublication.accepted then published,why=finishPublication.published,finishPublication.error
        else published,why=safe_emit()end
        return {outcome=entry.outcome,number=c.number,digest=c.digest,published=published,publicationError=why}
    end
    local function cancel(c)
        if not task or task.id~=c.task or task.digest~=c.taskDigest then return response(c,'task_identity_mismatch') end
        if task.phase=='terminal' then return response(c,'task_already_terminal',task.resultRef) end
        task.cancelRequested=true
        if type(a.cancel_execution)~='function' then return response(c,'cancel_unconfirmed') end
        local t=task
        local before=inspect(t)
        if before==true then finish(t.id,'unavailable','cancelled_execution_not_a_success');return response(c,'cancel_stop_confirmed',t.resultRef) end
        if before==nil then return response(c,'cancel_unconfirmed') end
        local time=now()
        if t.cleanupDeadline==0 then if not time or time>4294962295 then return response(c,'clock_unavailable') end;t.cleanupDeadline=time+CLEANUP_MILLIS end
        if not time or time>t.cleanupDeadline or t.cleanupAttempts>=MAX_CLEANUP_ATTEMPTS then return response(c,'cancel_budget_exhausted') end
        t.cleanupAttempts=t.cleanupAttempts+1
        local ok,confirmed=pcall(a.cancel_execution,t.owner)
        if task~=t or t.phase=='terminal' then return response(c,'task_already_terminal',t.resultRef) end
        local closed=ok and confirmed==true and inspect(t) or nil
        if closed==true then finish(t.id,'unavailable','cancelled_execution_not_a_success');return response(c,'cancel_stop_confirmed',t.resultRef) end
        return response(c,'cancel_unconfirmed')
    end
    local api={}
    function api.eligible(c)
        if c.number<=highWater then local old=history[c.number]
            return old~=nil and old.digest==c.digest,old and 'duplicate_or_conflict' or 'stale_outcome_unavailable'
        end
        if closed then return false,'connection_closed' end
        if c.op=='RUN' then return task==nil and now()~=nil,task and 'task_busy' or 'ready'
        elseif c.op=='RECEIVED' then
            return task~=nil and task.phase=='terminal' and task.id==c.task and task.digest==c.taskDigest and task.resultRef==c.resultRef,'result_identity'
        elseif c.op=='CANCEL' then return task~=nil and task.id==c.task and task.digest==c.taskDigest,'task_identity'
        elseif c.op=='CLOSE' then return task==nil,task and 'task_busy' or 'ready' end
        return false,'operation_unknown'
    end
    function api.dispatch(c)
        assert(type(c)=='table' and type(c.number)=='number' and c.number%1==0 and c.number>=1 and c.number<=4294967295
            and hex(c.digest,64) and type(c.task)=='number' and c.task%1==0 and c.task>=0,'exact_transport_command')
        if c.number<=highWater then
            local entry=history[c.number]
            if entry and entry.digest~=c.digest then return {outcome='command_conflict',published=false} end
            if not entry then return {outcome='stale_outcome_unavailable',published=false} end
            lastCommand=entry;local published,error=safe_emit()
            return {outcome=entry.outcome,number=entry.number,digest=entry.digest,published=published,publicationError=error,duplicate=true}
        end
        if closed then return response(c,'connection_closed') end
        if c.op=='RUN' then return run(c)
        elseif c.op=='RECEIVED' then return received(c)
        elseif c.op=='CANCEL' then return cancel(c)
        elseif c.op=='CLOSE' then
            if task then return response(c,'task_busy') end
            closed=true
            return response(c,'closed',completion and completion.resultRef or '')
        end
        return response(c,'operation_unknown')
    end
    function api.tick()
        if task and (task.phase=='running' or task.phase=='running_unknown') then local time=now()
            if not time or time>task.executionDeadline then
                local t=task
                if not t.timeoutStopAttempted then
                    t.timeoutStopAttempted=true
                    if inspect(t)~=true and type(a.cancel_execution)=='function' and t.cleanupAttempts<MAX_CLEANUP_ATTEMPTS then
                        t.cleanupAttempts=t.cleanupAttempts+1
                        pcall(a.cancel_execution,t.owner)
                    end
                end
                if task~=t or t.phase=='terminal' then return true end
                if inspect(t)==true then return finish(t.id,'unavailable','execution_timeout_stopped_outcome_unknown') end
                t.phase,t.resultState='running_unknown','unavailable';safe_emit();return true,'execution_timeout_unconfirmed'
            end
        end
        return false
    end
    function api.republish() return safe_emit() end
    function api.snapshot() return snapshot() end
    -- Strings are immutable; this does not expose the private task table.
    function api.body() return task and task.body or nil end
    return api
end
return M
