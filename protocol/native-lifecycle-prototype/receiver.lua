-- One real Lua51 business receiver shared by two/four-slot fixtures.
-- Host never invokes business.dispatch directly: every semantic request enters
-- through STAGE -> once-loader -> exact file/token COMMIT callbacks.
local root,slots,transport=assert(arg[1]),assert(arg[2]),assert(arg[3])
assert(transport=='four' or transport=='two')
local crypto=assert(loadfile(root..'/protocol/native-v1-prototype/crypto.lua'))()
local observe=assert(loadfile(root..'/protocol/native-v1-prototype/observation.lua'))()
local business=assert(loadfile(root..'/protocol/native-lifecycle-prototype/business.lua'))()
local commands=assert(loadfile(root..'/protocol/native-lifecycle-prototype/commands.lua'))()
local binding,runtime,session=string.rep('1',32),string.rep('2',32),string.rep('3',32)
local key=string.rep(string.char(0x19),32)
local peer=observe.new(crypto,key,binding,runtime,session)
local box,resources,loaded={},{},{}
local currentResourceOwner=nil
local clock,combat,cursor=10000,false,1
local highestClock=clock
local pending,focus,buffer=nil,false,''
local faults={};local async=false
local stats={codeCalls=0,closeCalls=0,closeEffects=0,cancelCalls=0,cancelEffects=0,bodyClears=0,focusReleases=0,slotLoads=0}
local function emit(value)io.write(value,'\n');io.flush()end
local function hex(s,n)return type(s)=='string' and #s==n and not s:find('[^0-9a-f]')end
local function unhex(s)if #s%2~=0 or s:find('[^0-9a-f]')then return nil end;return(s:gsub('..',function(p)return string.char(tonumber(p,16))end))end
local function split(s,sep)local f={};for x in(s..sep):gmatch('(.-)'..sep)do f[#f+1]=x end;return f end
local function digest_or_zero(s)return s~='' and s or string.rep('0',64)end
local function consume_fault(name)if faults[name]then faults[name]=nil;return true end;return false end
local function valid_clock()
    if type(clock)~='number'or clock%1~=0 or clock<highestClock or clock>4294966544 then return nil end
    highestClock=clock;return clock
end
local function release_focus()if focus then stats.focusReleases=stats.focusReleases+1 end;focus,buffer=false,'' end
local function complete_resource(r)
    if r.closed or not r.executing then return false end
    local fn,why=loadstring(r.code,'=lifecycle-owned-fixture')
    if not fn then r.executing=false;r.complete('unavailable','fixture_compile_error');return true end
    stats.codeCalls=stats.codeCalls+1
    local ok,value=pcall(fn);r.executing=false
    local complete=r.complete;r.code,r.complete=nil,nil
    complete(ok and 'verified' or 'unavailable',ok and tostring(value) or 'fixture_execution_error')
    return true
end
local function publish_state(s)
    if consume_fault('state_nil_before')then return nil end
    if consume_fault('state_false_before')then return false end
    if consume_fault('state_throw_before')then error('state_throw_before')end
    local raw=table.concat({'LDBC1',binding,runtime,session,string.format('%08x',s.revision),string.format('%08x',s.highWater),s.closed and '1'or'0',
        string.format('%08x',s.commandNumber),digest_or_zero(s.commandDigest),s.commandOutcome,string.format('%08x',s.commandTask),digest_or_zero(s.commandResultRef),
        string.format('%08x',s.task),digest_or_zero(s.taskDigest),s.phase,s.resultState,digest_or_zero(s.resultRef),s.bodyPresent and '1'or'0',string.format('%08x',s.bodyBytes),
        string.format('%08x',s.executionDeadline),s.cleanup,string.format('%02x',s.cleanupAttempts),string.format('%02x',s.bodyReleaseAttempts),string.format('%08x',s.cleanupDeadline),
        string.format('%08x',s.completionTask),digest_or_zero(s.completionResultRef),string.format('%08x',s.completionReceivedNumber),digest_or_zero(s.completionReceivedDigest),s.completionResultState},'|')
    local wire=raw..'|'..crypto.hmac(key,'lycheedev.lifecycle.state\0'..raw)
    rawset(box,'STATE',wire)
    if consume_fault('state_nil_after')then return nil end
    if consume_fault('state_false_after')then return false end
    if consume_fault('state_throw_after')then error('state_throw_after')end
    return rawget(box,'STATE')==wire
end
local receiver=business.new({clock=valid_clock,sha256=crypto.sha256,
    execute=function(owner,code,budget,complete)
        if consume_fault('execute_nil_before')then return nil end
        if consume_fault('execute_throw_before')then error('execute_throw_before')end
        if consume_fault('execute_false_before')then return false end
        if currentResourceOwner then local old=resources[currentResourceOwner]
            if old and old.closed and not old.executing then resources[currentResourceOwner]=nil end
        end
        assert(not resources[owner],'duplicate_execution_owner')
        local r={owner=owner,closed=false,executing=true,code=code,complete=complete};resources[owner]=r;currentResourceOwner=owner
        if consume_fault('execute_nil_after')then return nil end
        if consume_fault('execute_false_after')then return false end
        if consume_fault('execute_launch_throw')then error('execute_launch_throw')end
        if not async then complete_resource(r)end
        return true
    end,
    inspect_resources=function(owner)local r=resources[owner];if not r then return nil end;return{owner=r.owner,closed=r.closed and not r.executing}end,
    close_resources=function(owner)
        stats.closeCalls=stats.closeCalls+1;local r=assert(resources[owner],'owned_resource_missing')
        if consume_fault('cleanup_nil_before')then return nil end
        if consume_fault('cleanup_false_before')then return false end
        if consume_fault('cleanup_throw_before')then error('cleanup_throw_before')end
        if not r.closed then r.closed,r.executing=true,false;r.code,r.complete=nil,nil;stats.closeEffects=stats.closeEffects+1 end
        if consume_fault('cleanup_nil_after')then return nil end
        if consume_fault('cleanup_false_after')then return false end
        if consume_fault('cleanup_throw_after')then error('cleanup_throw_after')end
        return r.closed and not r.executing
    end,
    cancel_execution=function(owner)
        stats.cancelCalls=stats.cancelCalls+1;local r=assert(resources[owner],'owned_resource_missing')
        if consume_fault('cancel_nil_before')then return nil end
        if consume_fault('cancel_false_before')then return false end
        if consume_fault('cancel_throw_before')then error('cancel_throw_before')end
        if not r.closed then r.closed,r.executing=true,false;r.code,r.complete=nil,nil;stats.cancelEffects=stats.cancelEffects+1 end
        if consume_fault('cancel_nil_after')then return nil end
        if consume_fault('cancel_false_after')then return false end
        if consume_fault('cancel_throw_after')then error('cancel_throw_after')end
        return r.closed and not r.executing
    end,
    publish_body=function(task,ref,body)
        if consume_fault('body_nil_before')then return nil end
        if consume_fault('body_false_before')then return false end
        if consume_fault('body_throw_before')then error('body_throw_before')end
        rawset(box,'BODY',body)
        if consume_fault('body_nil_after')then return nil end
        if consume_fault('body_false_after')then return false end
        if consume_fault('body_throw_after')then error('body_throw_after')end
        -- Kernel already hashed this immutable private string at completion.
        -- Verify publication's owned reference; do not rehash 512KiB on every
        -- duplicate/retry. Host still hashes the actual read bytes independently.
        return rawget(box,'BODY')==body
    end,
    clear_body=function(task,ref)
        if consume_fault('clear_nil_before')then return nil end
        if consume_fault('clear_false_before')then return false end
        if consume_fault('clear_throw_before')then error('clear_throw_before')end
        rawset(box,'BODY',nil);stats.bodyClears=stats.bodyClears+1
        if consume_fault('clear_nil_after')then return nil end
        if consume_fault('clear_false_after')then return false end
        if consume_fault('clear_throw_after')then error('clear_throw_after')end
        return rawget(box,'BODY')==nil
    end,publish_state=publish_state,
})
local function once(slot)
    if slot<1 or slot>500 then return nil,'capacity'end
    if loaded[slot]then return nil,'already_loaded'end
    loaded[slot]=true;stats.slotLoads=stats.slotLoads+1
    local fn=loadfile(slots..'/slot-'..string.format('%03d',slot)..'.lua');if not fn then return nil,'empty_slot'end
    local ok,raw=pcall(fn);if not ok or type(raw)~='string' or #raw>1048704 then return nil,'file_limit'end
    return raw
end
local function stage()
    if pending then local time=valid_clock()
        if not time or time>pending.deadline then pending=nil;release_focus()else return 'ERR|pending'end
    end
    local slot=cursor;cursor=cursor+(transport=='four'and 2 or 1)
    local raw,why=once(slot);if not raw then return 'ERR|'..why end
    local f=split(raw,'|')
    if #f~=7 or f[1]~='P3S1'or f[2]~=binding or not hex(f[3],64)or f[4]~=string.format('%03d',slot)
        or f[5]~=string.format('%03d',transport=='four'and slot+1 or 0)or not hex(f[6],64)then return 'ERR|stage_fields'end
    local wire=unhex(f[7]);local c,error=commands.decode(wire,crypto);if not c or c.digest~=f[6]then return 'ERR|'..(error or 'logical_digest')end
    local time=valid_clock()
    pending={command=c,nonce=f[3],stage=slot,commit=transport=='four'and slot+1 or 0,deadline=time and time+750 or 0}
    local allowed=receiver.eligible(c);local facts=allowed and time and 0 or 8
    peer.controller.set(combat,false,facts~=0,time or 0)
    if transport=='two'then focus,buffer=true,''end
    return 'STATE|'..peer.stage(c.number,pending.nonce,c.digest)
end
local function dispatch_exact()
    local p=pending;pending=nil;release_focus()
    if not p then return 'ERR|no_pending'end
    local time=valid_clock()
    if combat or not time or time>p.deadline or not receiver.eligible(p.command)then return 'ERR|private_gate'end
    local result=receiver.dispatch(p.command)
    return 'DISPATCH|'..result.outcome..'|'..(result.published and 'published'or'unconfirmed')
end
local function commit_file()
    if transport~='four'or not pending then return 'ERR|no_file_pending'end
    local time=valid_clock()
    if combat or not time or time>pending.deadline then pending=nil;release_focus();return 'ERR|private_gate'end
    local raw,why=once(pending.commit);if not raw then pending=nil;return 'ERR|'..why end
    local expected=table.concat({'P3C1',binding,pending.nonce,string.format('%03d',pending.stage),string.format('%03d',pending.commit),pending.command.digest},'|')
    if raw~=expected then pending=nil;return 'ERR|commit_exact'end
    return dispatch_exact()
end
local function text_changed(text)
    if transport~='two'or not focus then return 'ERR|no_focus'end
    if #text>255 or text:find('[^0-9A-Za-z:]')then release_focus();pending=nil;return 'ERR|text_encoding'end
    buffer=text;return 'TEXT|inert'
end
local function enter()
    if transport~='two'or not focus or not pending then release_focus();return 'ERR|no_token_pending'end
    local f=split(buffer,':');local p=pending
    if #f~=8 or f[1]~='P3T1'or f[2]~=binding or f[3]~=p.nonce or f[4]~=string.format('%08x',p.command.number)or f[5]~=p.command.digest
        or f[6]~=string.format('%03d',p.stage)or f[7]~=string.format('%08x',p.deadline)or not hex(f[8],64)then release_focus();pending=nil;return 'ERR|token_exact'end
    local raw=table.concat({f[2],f[3],f[4],f[5],f[6],f[7]},':')
    if crypto.hmac(key,'lycheedev.lifecycle.token\0'..raw)~=f[8]then release_focus();pending=nil;return 'ERR|token_mac'end
    return dispatch_exact()
end
receiver.republish();emit('READY|Lua51|shared-business|transport-'..transport)
local count=0
for line in io.lines()do
    count=count+1;if count>4096 or #line>8192 then emit('ERR|input_budget');break end
    if line=='STAGE'then emit(stage())
    elseif line=='COMMIT'then emit(commit_file())
    elseif line=='ENTER'then emit(enter())
    elseif line:match('^TEXT|')then emit(text_changed(string.sub(line,6)))
    elseif line=='MAILBOX'then emit(rawget(box,'STATE')or 'MISSING')
    elseif line=='BODY'then local body=rawget(box,'BODY');emit(body and 'BODY|'..(body:gsub('.',function(c)return string.format('%02x',string.byte(c))end))or'MISSING')
    elseif line=='REPUBLISH'then local ok=receiver.republish();emit(ok and 'OK|republished'or'ERR|publication')
    elseif line=='DONE'then local completed=false;for _,r in pairs(resources)do if complete_resource(r)then completed=true end end;emit(completed and 'OK|completed'or'OK|no_execution')
    elseif line=='ASYNC_ON'then async=true;emit('OK|async')
    elseif line=='COMBAT_ON'then combat=true;release_focus();pending=nil;emit('OK|combat')
    elseif line=='COMBAT_OFF'then combat=false;emit('OK|combat_off')
    elseif line=='FOCUS_LOST'or line=='ESCAPE'then release_focus();pending=nil;emit('OK|released')
    elseif line:match('^CLOCK|%d+$')then clock=tonumber(line:match('%d+$'));receiver.tick();emit('OK|clock_model')
    elseif line:match('^FAULT|[a-z_]+$')then faults[string.sub(line,7)]=true;emit('OK|fault')
    elseif line=='PUBLIC_BODY_TAMPER'then rawset(box,'BODY','forged-body');emit('OK|public_tamper')
    elseif line=='PUBLIC_STATE_TAMPER'then rawset(box,'STATE','forged-state');emit('OK|public_tamper')
    elseif line=='STATUS'then local s=receiver.snapshot();emit(table.concat({'STATUS',s.phase,s.bodyPresent and '1'or'0',s.cleanup,tostring(s.cleanupAttempts),tostring(s.cleanupDeadline),
        tostring(stats.codeCalls),tostring(stats.closeCalls),tostring(stats.closeEffects),tostring(stats.cancelCalls),tostring(stats.cancelEffects),tostring(stats.bodyClears),tostring(stats.slotLoads),tostring(stats.focusReleases)},'|'))
    elseif line=='EXIT'then release_focus();emit('EXIT');break
    else emit('ERR|fixture_command')end
end
