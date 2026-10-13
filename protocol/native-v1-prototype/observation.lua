-- Isolated P0 receiver, not the final protocol or a WoW input implementation.
-- Test controller may set private facts; attacker only receives the public box.
local M={}
function M.new(crypto,key,binding,runtime,session)
    local public={}
    local revision,clock,combat,focus,loading=40,10000,false,false,false
    local pending=nil
    local function publish()
        assert(pending,"no_pending")
        revision=revision+1
        local flags=(combat and 1 or 0)+(focus and 2 or 0)+(loading and 4 or 0)
        local raw=table.concat({"LDP1",binding,runtime,session,string.format('%08x',pending.number),pending.nonce,pending.digest,
            string.format('%08x',revision),string.format('%08x',clock),string.format('%02x',flags)},'|')
        public.STATE=raw..'|'..crypto.hmac(key,'lycheedev.p0.state\0'..raw)
        return public.STATE
    end
    local function stage(number,nonce,digest)
        assert(type(number)=='number' and number>=1 and number<=4294967295 and number==math.floor(number),'number')
        assert(type(nonce)=='string' and #nonce==64 and not nonce:find('[^0-9a-f]'),'nonce')
        assert(type(digest)=='string' and #digest==64 and not digest:find('[^0-9a-f]'),'digest')
        pending={number=number,nonce=nonce,digest=digest}
        return publish()
    end
    -- No generic sign method is exposed by the receiver.
    return {
        public=public,stage=stage,
        controller={set=function(c,f,l,t) combat,focus,loading,clock=c,f,l,t end,republish=publish},
    }
end
return M
