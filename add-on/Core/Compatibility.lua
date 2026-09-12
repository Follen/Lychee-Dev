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

-- UI units per physical screen pixel, for screen-space overlays such as the
-- automation status blocks and QR notice. GetPhysicalScreenSize was verified on
-- the Retail 12.1.0 baseline only; other clients use the legacy 768-height
-- approximation until verified.
function client.GetPhysicalPixelSize()
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
    -- Fallback for clients without a verified GetPhysicalScreenSize: the legacy
    -- 768-height approximation treats the screen as 768 physical pixels tall,
    -- so one physical pixel spans (uiHeight / 768) UI units. The inverse,
    -- 768 / uiHeight, would shrink the 4x4 physical-pixel status blocks below
    -- their specified minimum on tall UIs.
    local uiHeight = GetScreenHeight and GetScreenHeight() or nil
    if uiHeight and uiHeight > 0 and not (issecretvalue and issecretvalue(uiHeight)) then
        return uiHeight / 768
    end
    return 1
end
