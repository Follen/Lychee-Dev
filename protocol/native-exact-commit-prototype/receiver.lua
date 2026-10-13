-- Isolated Lua 5.1 receiver. Physical buttons always go through load_once;
-- no direct Receive entry is provided to the host. This is not a WoW adapter.
local root,slots=assert(arg[1]),assert(arg[2])
local crypto=assert(loadfile(root..'/protocol/native-v1-prototype/crypto.lua'))()
local observe=assert(loadfile(root..'/protocol/native-v1-prototype/observation.lua'))()
local binding,runtime,session=string.rep('1',32),string.rep('2',32),string.rep('3',32)
local peer=observe.new(crypto,string.rep(string.char(0x19),32),binding,runtime,session)
local loaded,pending,executions={},nil,{}
local cursor,lastStage,lastAccepted,combat=1,0,0,false
local callbacks,loads=0,0
local function emit(value) io.write(value,'\n');io.flush() end
local function hex(s,n) return type(s)=='string' and #s==n and not s:find('[^0-9a-f]') end
local function parse(wire,kind)
    if type(wire)~='string' or #wire>4096 then return nil,'wire_size' end
    local f={};for v in (wire..'|'):gmatch('(.-)|') do f[#f+1]=v end
    if #f~=9 or f[1]~=kind or not hex(f[2],32) or not hex(f[3],8)
        or not hex(f[4],64) or not hex(f[5],64) or not f[6]:match('^%d%d%d$')
        or not f[7]:match('^%d%d%d$') or (f[8]~='RUN' and f[8]~='RECEIVED')
        or #f[9]>2048 or #f[9]%2~=0 or f[9]:find('[^0-9a-f]') then return nil,'wire_fields' end
    local n,s,c=tonumber(f[3],16),tonumber(f[6]),tonumber(f[7])
    if n==0 or s<1 or c>500 or c~=s+1 or s%2~=1 then return nil,'wire_slots' end
    local raw=table.concat({f[2],f[3],f[4],f[6],f[7],f[8],f[9]},'|')
    if crypto.sha256(raw)~=f[5] then return nil,'wire_digest' end
    return {binding=f[2],number=n,nonce=f[4],digest=f[5],stage=s,commit=c,op=f[8],payload=f[9],raw=raw}
end
local function load_once(slot,kind)
    if slot<1 or slot>500 then return nil,'capacity' end
    if loaded[slot] then return nil,'already_loaded' end
    loaded[slot]=true;loads=loads+1
    -- All 500 LoD wrappers are assumed installed. An empty wrapper also loads
    -- once; this models that failure honestly rather than silently reusing it.
    local fn=loadfile(slots..'/slot-'..string.format('%03d',slot)..'.lua')
    if not fn then return nil,'empty_slot' end
    local ok,wire=pcall(fn);if not ok then return nil,'slot_exception' end
    return parse(wire,kind)
end
local function stage_button()
    callbacks=callbacks+1
    if pending then return 'ERR|pending' end
    if cursor>499 then return 'ERR|capacity' end
    local expected=cursor;cursor=cursor+2
    local x,why=load_once(expected,'STAGE')
    if not x then return 'ERR|'..why end
    if x.binding~=binding or x.stage~=expected then return 'ERR|stage_binding_or_slot' end
    if x.number<=lastStage then return 'ERR|stage_sequence' end
    lastStage=x.number;pending=x
    peer.controller.set(combat,false,false,10000)
    return 'STATE|'..peer.stage(x.number,x.nonce,x.digest)
end
local function commit_button()
    callbacks=callbacks+1
    if not pending then return 'ERR|no_pending' end
    -- The private condition is checked before loading AND before dispatch.
    if combat then return 'ERR|private_blocked' end
    local x,why=load_once(pending.commit,'COMMIT')
    if not x then pending=nil;return 'ERR|'..why end
    if x.raw~=pending.raw or x.digest~=pending.digest or x.number<=lastAccepted then
        pending=nil;return 'ERR|commit_exact_mismatch'
    end
    if combat then pending=nil;return 'ERR|private_blocked' end
    lastAccepted=x.number;executions[#executions+1]=x.number..':'..x.op..':'..x.payload
    pending=nil
    return 'ACCEPTED|'..string.format('%08x',x.number)..'|'..x.digest
end
emit('READY|Lua51|fixed_trusted_binding|once_loader_model')
local count=0
for line in io.lines() do
    count=count+1;if count>1024 or #line>8192 then emit('ERR|input_budget');break end
    if line=='STAGE' then emit(stage_button())
    elseif line=='COMMIT' then emit(commit_button())
    elseif line=='COMBAT_ON' then combat=true;emit('OK|combat_on')
    elseif line=='COMBAT_OFF' then combat=false;emit('OK|combat_off')
    elseif line=='STATUS' then emit('STATUS|'..#executions..'|'..loads..'|'..callbacks..'|'..table.concat(executions,','))
    elseif line=='EXIT' then emit('EXIT');break
    else emit('ERR|input_command') end
end
