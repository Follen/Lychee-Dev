local ADDON_NAME, ns = ...

-- One shared API surface for verified client differences needed by the
-- workbench. Every value that crosses a trust boundary is checked for secrecy
-- before it is compared, formatted, indexed or branched on; failures open to a
-- harmless visible state instead of throwing.
ns.Compat = {
    GetAddOnMetadata = function(addonName, field)
        if type(C_AddOns) ~= "table" or type(C_AddOns.GetAddOnMetadata) ~= "function" then
            return nil
        end
        if (issecretvalue and (issecretvalue(addonName) or issecretvalue(field))) then
            return nil
        end
        local succeeded, value = pcall(C_AddOns.GetAddOnMetadata, addonName, field)
        if not succeeded or (issecretvalue and issecretvalue(value)) then
            return nil
        end
        return value
    end,

    GetMouseFocus = function()
        if type(GetMouseFoci) ~= "function" then
            return nil
        end
        local succeeded, foci = pcall(GetMouseFoci)
        if not succeeded or (issecretvalue and issecretvalue(foci))
            or type(foci) ~= "table"
            or (issecretvalue and issecretvalue(foci[1])) or not foci[1] then
            return nil
        end
        return foci[1]
    end,

    -- UI units per physical screen pixel, for screen-space overlays. The
    -- caller multiplies by a physical-pixel target to get UI units.
    --
    -- Only the width ratio is trustworthy: GetScreenWidth and the physical
    -- width are both display measurements. GetScreenHeight is NOT usable as a
    -- fallback, because the engines disagree about what it returns (Retail
    -- reports the UIParent height, Classic the display height), so a ratio
    -- built from it inflates or shrinks an overlay by a large factor on exactly
    -- the clients that lack GetPhysicalScreenSize. When the physical width is
    -- unavailable this reports 1: one UI unit per pixel, the neutral answer.
    GetPhysicalPixelSize = function()
        if GetPhysicalScreenSize and GetScreenWidth then
            local succeeded, width = pcall(GetPhysicalScreenSize)
            local uiWidth = GetScreenWidth()
            if succeeded and not (issecretvalue and (issecretvalue(width) or issecretvalue(uiWidth)))
                and type(width) == "number" and width > 0 and width < math.huge
                and type(uiWidth) == "number" and uiWidth > 0 and uiWidth < math.huge then
                return math.min(uiWidth / width, 1)
            end
        end
        return 1
    end,
}
