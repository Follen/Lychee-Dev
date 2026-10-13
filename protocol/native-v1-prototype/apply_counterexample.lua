-- Actual Lua callbacks and once-only loader, not direct Receive(slot).
-- This deliberately demonstrates why P0 observation success is insufficient.
local function world()
    local files,loaded,pending,executions={}, {}, nil, {}
    local cursor=1
    local function load_once(index)
        assert(index>=1 and index<=500)
        if loaded[index] then return false end
        loaded[index]=true
        local value=files[index]
        if not value then return false end
        pending={nonce=value.nonce,digest=value.digest}
        return true
    end
    local buttons={}
    buttons.STAGE=function()
        local result=load_once(cursor)
        cursor=cursor+1
        return result
    end
    buttons.APPLY=function()
        if not pending then return false end
        executions[#executions+1]=pending.digest
        pending=nil
        return true
    end
    return {files=files,buttons=buttons,executions=executions,pending=function()return pending end}
end
local a=world()
a.files[1]={nonce='old',digest='A'}
assert(a.buttons.STAGE())
-- Host's APPLY(A) is queued, not consumed. A naive recovery publishes B.
a.files[2]={nonce='new',digest='B'}
assert(a.buttons.STAGE())
local hostSawBReceipt=false
assert(a.buttons.APPLY()) -- This is the old generic physical key.
assert(#a.executions==1 and a.executions[1]=='B' and not hostSawBReceipt)
print('OLD_APPLY_NEW_STAGE\tcounterexample_reproduced')
print('HOST_SAW_NEW_RECEIPT\tfalse')
local b=world()
-- An old STAGE callback arriving after future publication can consume it.
b.files[1]={nonce='future',digest='F'}
assert(b.buttons.STAGE())
assert(b.pending().digest=='F' and #b.executions==0)
print('OLD_STAGE_FUTURE_FILE\tstages_without_business')
-- Minimal host-side admission rule is executable here, not silently presumed.
local function can_publish(driver) return driver.unresolvedApply==0 and driver.intentDurable end
assert(not can_publish({unresolvedApply=1,intentDurable=true}))
assert(not can_publish({unresolvedApply=0,intentDurable=false}))
assert(can_publish({unresolvedApply=0,intentDurable=true}))
print('UNKNOWN_APPLY_NEW_STAGE\trejected_by_admission')
-- Losing the journal must not be represented by unresolvedApply=0.
local function restore(record)
    if not record then return nil,'quarantine' end
    return record
end
local missing,why=restore(nil)
assert(missing==nil and why=='quarantine')
print('MISSING_DRIVER_JOURNAL\tquarantine')
print('SCOPE\tLua callbacks and once-only loader;no_OS_queue_or_WoW_proof')
