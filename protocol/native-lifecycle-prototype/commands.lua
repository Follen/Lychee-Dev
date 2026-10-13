-- Canonical logical commands, shared by two/four-slot transport adapters.
-- Attempt nonce/physical slots are separate envelope fields, not business
-- identity. An exact logical retry cannot change code or reset its budget.
local M={MAX_WIRE=524416}
local function hex(s,n) return type(s)=='string' and #s==n and not s:find('[^0-9a-f]') end
local function bytes(s)
    if #s%2~=0 or s:find('[^0-9a-f]') then return nil end
    return (s:gsub('..',function(pair)return string.char(tonumber(pair,16))end))
end
function M.decode(wire,crypto)
    if type(wire)~='string' or #wire>M.MAX_WIRE then return nil,'command_size' end
    local f={};for v in (wire..'|'):gmatch('(.-)|') do f[#f+1]=v end
    if f[1]~='K1' or not hex(f[2],8) or not hex(f[4],8) then return nil,'command_fields' end
    local number,task=tonumber(f[2],16),tonumber(f[4],16)
    if number<1 then return nil,'command_number' end
    local c={number=number,task=task,op=f[3],digest=crypto.sha256(wire)}
    if c.op=='RUN' then
        if #f~=6 or task~=number or not hex(f[5],8) then return nil,'run_fields' end
        c.budgetMillis=tonumber(f[5],16);c.code=bytes(f[6]);c.taskDigest=c.digest
        if not c.code or #c.code>262144 or c.budgetMillis<1000 or c.budgetMillis>120000 then return nil,'run_fields' end
    elseif c.op=='RECEIVED' then
        if #f~=6 or task<1 or not hex(f[5],64) or not hex(f[6],64) then return nil,'received_fields' end
        c.taskDigest,c.resultRef=f[5],f[6]
    elseif c.op=='CANCEL' then
        if #f~=5 or task<1 or not hex(f[5],64) then return nil,'cancel_fields' end
        c.taskDigest=f[5]
    elseif c.op=='CLOSE' then
        if #f~=4 or task~=0 then return nil,'close_fields' end
    else return nil,'command_operation' end
    return c
end
return M
