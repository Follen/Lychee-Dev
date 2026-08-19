local ADDON_NAME, ns = ...

local client = assert(ns.Client, "Lychee Dev client profile must load first")

function client.GetNumAddOns()
    return C_AddOns.GetNumAddOns()
end

function client.GetAddOnInfo(addon)
    return C_AddOns.GetAddOnInfo(addon)
end

function client.IsAddOnLoaded(addon)
    local loaded = C_AddOns.IsAddOnLoaded(addon)
    return loaded and true or false
end

function client.GetAddOnMetadata(addon, field)
    return C_AddOns.GetAddOnMetadata(addon, field)
end

function client.GetMouseFocus()
    local foci = GetMouseFoci()
    if issecretvalue(foci) or type(foci) ~= "table" or not foci[1]
        or issecretvalue(foci[1]) then
        return nil
    end
    return foci[1]
end

function client.HasAddOnProfiler()
    return C_AddOnProfiler
        and type(C_AddOnProfiler.IsEnabled) == "function"
        and type(C_AddOnProfiler.GetAddOnMetric) == "function"
        and C_AddOnProfiler.IsEnabled()
        and true or false
end

function client.HasFunctionProfiler()
    return type(GetFunctionCPUUsage) == "function"
        and GetCVarBool("scriptProfile") and true or false
end

function client.HasFrameProfiler()
    return type(GetFrameCPUUsage) == "function"
        and GetCVarBool("scriptProfile") and true or false
end

function client.HasFrameEnumeration()
    return type(EnumerateFrames) == "function"
end
