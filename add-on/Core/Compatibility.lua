local ADDON_NAME, ns = ...

local client = assert(ns.Client, "Lychee Dev client profile must load first")

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
