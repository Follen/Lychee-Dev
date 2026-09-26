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
            or type(foci) ~= "table" or not foci[1]
            or (issecretvalue and issecretvalue(foci[1])) then
            return nil
        end
        return foci[1]
    end,

    -- UI units per physical screen pixel, for screen-space overlays such as
    -- the automation status blocks and QR notice. The caller multiplies by a
    -- physical-pixel target to get UI units, so a large value means "one
    -- physical pixel is worth many UI units" and would draw a huge overlay.
    --
    -- GetPhysicalScreenSize was verified on the Retail 12.1.0 baseline only.
    -- Elsewhere the UI height is the only scale available: a taller UI means
    -- more UI units per physical pixel is wrong, because the UI is measured in
    -- units that already track the display. The safe direction is to assume the
    -- UI is at most the reference 768-unit layout, i.e. uiHeight / 768 UI units
    -- per physical pixel, capped at one so a fallback can never inflate an
    -- overlay. The cap keeps this a floor on readability, not a floor on size.
    GetPhysicalPixelSize = function()
        if GetPhysicalScreenSize then
            local succeeded, width, height = pcall(GetPhysicalScreenSize)
            if succeeded and not (issecretvalue and (issecretvalue(width) or issecretvalue(height)))
                and width and width > 0 and GetScreenWidth then
                local uiWidth = GetScreenWidth()
                if uiWidth and uiWidth > 0 and not (issecretvalue and issecretvalue(uiWidth)) then
                    local ratio = uiWidth / width
                    if ratio > 0 and ratio < math.huge then
                        return math.min(ratio, 1)
                    end
                end
            end
        end
        local uiHeight = GetScreenHeight and GetScreenHeight() or nil
        if uiHeight and uiHeight > 0 and not (issecretvalue and issecretvalue(uiHeight)) then
            return math.min(uiHeight / 768, 1)
        end
        return 1
    end,
}
