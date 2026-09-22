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
    -- the automation status blocks and QR notice. GetPhysicalScreenSize was
    -- verified on the Retail 12.1.0 baseline only; other clients use the
    -- legacy 768-height approximation until verified.
    GetPhysicalPixelSize = function()
        if GetPhysicalScreenSize then
            local succeeded, width, height = pcall(GetPhysicalScreenSize)
            if succeeded and not (issecretvalue and (issecretvalue(width) or issecretvalue(height)))
                and width and width > 0 and GetScreenWidth then
                local uiWidth = GetScreenWidth()
                if uiWidth and uiWidth > 0 and not (issecretvalue and issecretvalue(uiWidth)) then
                    return uiWidth / width
                end
            end
        end
        -- Fallback for clients without a verified GetPhysicalScreenSize: the
        -- legacy 768-height approximation treats the screen as 768 physical
        -- pixels tall, so one physical pixel spans (uiHeight / 768) UI units.
        local uiHeight = GetScreenHeight and GetScreenHeight() or nil
        if uiHeight and uiHeight > 0 and not (issecretvalue and issecretvalue(uiHeight)) then
            return uiHeight / 768
        end
        return 1
    end,
}
