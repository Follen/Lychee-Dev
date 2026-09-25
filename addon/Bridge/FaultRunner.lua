local ADDON_NAME, ns = ...

local function restricted(value) return issecretvalue and issecretvalue(value) end
local function requestKey(value)
    return not restricted(value) and type(value)=="string" and #value>0 and #value<=128
        and string.match(value,"^[%w_%-]+$")~=nil
end

ns.FaultRunner={
    Run=function(requestId,count)
        if not requestKey(requestId) or restricted(count) or type(count)~="number"
            or count%1~=0 or count<1 or count>100 then return nil,"fault_invalid_request" end
        local session,reason=ns.Session.Current()
        if not session then return nil,reason end
        local receipt,missing=ns.ReportStore.Read(requestId)
        if receipt or missing~="report_unavailable" then return nil,"fault_request_exists" end
        if type(ns.Diagnostics)~="table" or type(ns.Diagnostics.SnapshotRecentErrors)~="function" then
            return nil,"fault_diagnostics_unavailable"
        end
        local snapshot,failure=ns.Diagnostics.SnapshotRecentErrors(count,"provider_storage")
        local body={schema="lycheedev.bugs.v1",requestType="bugs",
            status=snapshot and "completed" or "unavailable",complete=snapshot~=nil,
            snapshot=snapshot,error=failure}
        return ns.ReportStore.Commit(requestId,"",body)
    end,
    Acknowledge=function(requestId,sequence)
        if not requestKey(requestId) or restricted(sequence) or type(sequence)~="number"
            or sequence%1~=0 or sequence<1 or sequence>9007199254740991 then
            return nil,"fault_invalid_acknowledgement"
        end
        local session,reason=ns.Session.Current()
        if not session then return nil,reason end
        local receipt,body=ns.ReportStore.Read(requestId)
        if not receipt then return nil,body end
        return ns.ReportStore.Acknowledge({schema="lycheedev.signal.v1",release=ns.Release,
            kind="reported",sessionNonce=session.sessionNonce,requestId=requestId,
            character=session.character,realm=session.realm,sequence=sequence,
            product=ns.Startup.identity.product,build=ns.Startup.identity.build,
            inputReady=false,reportBytes=#body,reportAdler32=ns.CaptureWriter.DigestBytes(body)})
    end,
}
