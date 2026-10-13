local _,ns=...
-- Candidate adapter: bound input loads exactly one own-lane module, never foreign lanes.
ns.NativeRuntime={Create=function(a)
    local protocol=ns.NativeProtocol.Create(a)
    local loading,received,control=false,false,false
    local api={Protocol=protocol}
    local function load(controlAction)
        if loading then return nil,"native_reentrant_wake"end
        if controlAction=="settle" then if not protocol.CanSettle()then return nil,"native_result_not_ready"end
        elseif controlAction=="close" then if not protocol.CanClose()then return nil,"native_request_pending"end
        elseif not protocol.CanExecuteWake()then return nil,"native_execute_not_ready"end
        local slot=protocol.NextSlot();if not slot or slot>500 then return nil,"native_route_unbound"end
        loading,received,control=true,false,controlAction~=nil
        local ok,result,reason=pcall(a.load,string.format("Lychee Dev Slot %02d",slot))
        loading=false
        if not received then protocol.Receive(slot,nil,false,control)end
        control=false
        if not ok then return nil,"native_load_exception"end
        return result,reason
    end
    function api.Wake()return load(nil)end
    function api.Settle()return load("settle")end
    function api.Close()return load("close")end
    function api.Receive(slot,e,bootstrap)
        if not bootstrap and not loading then return nil,"native_unsolicited_loader"end
        received=true;return protocol.Receive(slot,e,bootstrap,control)
    end
    return api
end}
