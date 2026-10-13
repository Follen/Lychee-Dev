-- Two-slot candidate: one read-only LoD STAGE per logical command, followed
-- by an authenticated exact token carried by modeled physical text callbacks.
-- Not an installed WoW UI; focus/key/clock behavior below is a declared model.
local root,slots=assert(arg[1]),assert(arg[2])
local crypto=assert(loadfile(root..'/protocol/native-v1-prototype/crypto.lua'))()
local observe=assert(loadfile(root..'/protocol/native-v1-prototype/observation.lua'))()
local binding,runtime,session=string.rep('1',32),string.rep('2',32),string.rep('3',32)
local key=string.rep(string.char(0x19),32)
local peer=observe.new(crypto,key,binding,runtime,session)
local loaded,pending,executions={},nil,{}
local nextSlot,lastStage,lastAccepted,clock=1,0,0,10000
local active,buffer,combat=false,'',false
local loads,releaseCount,keyCallbacks,revision=0,0,0,0
local mailbox,lastAttempt
local MAX_TOKEN=255
local function emit(value) io.write(value,'\n');io.flush() end
local function hex(s,n) return type(s)=='string' and #s==n and not s:find('[^0-9a-f]') end
local function receipt(outcome,consumed,x)
    x=x or lastAttempt;revision=revision+1
    local raw=table.concat({'LDTM1',binding,runtime,session,string.format('%08x',x and x.number or 0),x and x.nonce or string.rep('0',64),
        x and x.digest or string.rep('0',64),string.format('%03d',x and x.slot or 0),outcome,consumed and '1' or '0',
        string.format('%08x',clock),string.format('%08x',x and x.deadline or 0),tostring(#executions),tostring(loads),tostring(releaseCount),tostring(revision)},'|')
    mailbox=raw..'|'..crypto.hmac(key,'lycheedev.two-slot.receipt\0'..raw)
end
local function release()
    if active then releaseCount=releaseCount+1 end
    active,buffer=false,''
end
local function split(s,delimiter)
    local out={};for v in (s..delimiter):gmatch('(.-)'..delimiter) do out[#out+1]=v end;return out
end
local function load_once(slot)
    if slot<1 or slot>500 then return nil,'capacity' end
    if loaded[slot] then return nil,'already_loaded' end
    loaded[slot]=true;loads=loads+1
    local fn=loadfile(slots..'/slot-'..string.format('%03d',slot)..'.lua');if not fn then return nil,'empty_slot' end
    local ok,wire=pcall(fn);if not ok or type(wire)~='string' or #wire>4096 then return nil,'stage_size' end
    local f=split(wire,'|')
    if #f~=8 or f[1]~='STAGE' or not hex(f[2],32) or not hex(f[3],8) or not hex(f[4],64) or not hex(f[5],64)
        or not f[6]:match('^%d%d%d$') or (f[7]~='RUN' and f[7]~='RECEIVED') or #f[8]>2048 or #f[8]%2~=0 or f[8]:find('[^0-9a-f]') then return nil,'stage_fields' end
    local raw=table.concat({f[2],f[3],f[4],f[6],f[7],f[8]},'|')
    if crypto.sha256(raw)~=f[5] then return nil,'stage_digest' end
    return {binding=f[2],number=tonumber(f[3],16),nonce=f[4],digest=f[5],slot=tonumber(f[6]),op=f[7],payload=f[8]}
end
local function stage_button()
    keyCallbacks=keyCallbacks+1
    if pending then receipt('stage_pending',false,pending);return 'ERR|pending' end
    if combat then receipt('private_blocked',false);return 'ERR|private_blocked' end
    local slot=nextSlot;nextSlot=nextSlot+1
    local x,why=load_once(slot)
    if not x then receipt('stage_'..why,true);return 'ERR|'..why end
    if x.binding~=binding or x.slot~=slot or x.number<=lastStage then receipt('stage_identity_or_sequence',true);return 'ERR|stage_identity_or_sequence' end
    x.deadline=clock+750;lastStage=x.number;lastAttempt=x;pending=x
    -- Opening the modeled receiver adds no file. Real creation/focus legality
    -- must be established on the pinned UI APIs and final clients separately.
    active,buffer=true,''
    peer.controller.set(combat,false,false,clock);receipt('staged',true,x)
    return 'STATE|'..peer.stage(x.number,x.nonce,x.digest)
end
local function text_changed(text)
    if not active then return 'ERR|no_focus' end
    if type(text)~='string' or #text>MAX_TOKEN or text:find('[^0-9A-Za-z:]') then release();receipt('text_encoding_or_limit',false);pending=nil;return 'ERR|text_encoding_or_limit' end
    if #text<=5 and string.sub('LDT1:',1,#text)~=text then release();receipt('text_prefix',false);pending=nil;return 'ERR|text_prefix' end
    buffer=text
    return 'TEXT|inert' -- Not even a complete valid token dispatches here.
end
local function token()
    local f=split(buffer,':')
    if #f~=8 or f[1]~='LDT1' or not hex(f[2],32) or not hex(f[3],8) or not hex(f[4],64) or not hex(f[5],64)
        or not f[6]:match('^%d%d%d$') or not hex(f[7],8) or not hex(f[8],64) then return nil,'token_fields' end
    local raw=table.concat({f[2],f[3],f[4],f[5],f[6],f[7]},':')
    if crypto.hmac(key,'lycheedev.two-slot.commit\0'..raw)~=f[8] then return nil,'token_mac' end
    return {binding=f[2],number=tonumber(f[3],16),nonce=f[4],digest=f[5],slot=tonumber(f[6]),deadline=tonumber(f[7],16)}
end
local function enter_key()
    keyCallbacks=keyCallbacks+1
    if not active or not pending then release();receipt('no_active_pending',false);return 'ERR|no_active_pending' end
    if combat then release();receipt('private_blocked',false,pending);pending=nil;return 'ERR|private_blocked' end
    local x,why=token();if not x then release();receipt(why,false,pending);pending=nil;return 'ERR|'..why end
    local p=pending
    if x.binding~=p.binding or x.number~=p.number or x.nonce~=p.nonce or x.digest~=p.digest or x.slot~=p.slot or x.deadline~=p.deadline or x.number<=lastAccepted then
        release();receipt('token_exact_mismatch',false,p);pending=nil;return 'ERR|token_exact_mismatch'
    end
    if clock>p.deadline then release();receipt('private_expired',false,p);pending=nil;return 'ERR|private_expired' end
    -- Release focus BEFORE any business effect, and record only exact request.
    release();lastAccepted=x.number;executions[#executions+1]=p.number..':'..p.op..':'..p.payload
    receipt('accepted',false,p);pending=nil
    return 'ACCEPTED|'..string.format('%08x',x.number)..'|'..x.digest
end
receipt('idle',false)
emit('READY|Lua51|two-slot-token-entry-model')
local count=0
for line in io.lines() do
    count=count+1;if count>2048 or #line>8192 then emit('ERR|fixture_input_budget');break end
    if line=='STAGE' then emit(stage_button())
    elseif line=='ENTER' then emit(enter_key())
    elseif line:match('^TEXT|') then emit(text_changed(string.sub(line,6)))
    elseif line:match('^CHAR|') then local c=string.sub(line,6);keyCallbacks=keyCallbacks+1;if #c~=1 then emit('ERR|fixture_char')else emit(text_changed(buffer..c))end
    elseif line=='ESCAPE' or line=='FOCUS_LOST' then keyCallbacks=keyCallbacks+1;release();receipt('input_released',false);pending=nil;emit('OK|released')
    elseif line=='COMBAT_ON' then combat=true;release();receipt('private_blocked',false);pending=nil;emit('OK|combat_on')
    elseif line=='COMBAT_OFF' then combat=false;emit('OK|combat_off')
    elseif line:match('^CLOCK|%d+$') then local t=tonumber(line:match('%d+$'));if t<clock or t>4294966544 then emit('ERR|fixture_clock')else clock=t;emit('OK|clock_model')end
    elseif line=='MAILBOX' then emit(mailbox)
    elseif line=='STATUS' then emit('STATUS|'..#executions..'|'..loads..'|'..releaseCount..'|'..keyCallbacks..'|'..(active and 'focused' or 'released'))
    elseif line=='EXIT' then release();emit('EXIT');break
    else emit('ERR|fixture_command') end
end
