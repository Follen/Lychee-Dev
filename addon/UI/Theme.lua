local ADDON_NAME, ns = ...

-- Visual constants from Design.md. Loading this table creates no UI objects.
ns.Theme = {
    window = { .055, .055, .063 },
    surfaceHover = { .090, .090, .090 },
    surfaceSelected = { .085, .085, .085 },
    field = { .085, .085, .095 },
    fieldBorder = { .400, .400, .420 },
    sidebar = { .075, .073, .078 },
    action = { .145, .125, .135 },
    controlRadius = 8,
    panelRadius = 12,
    text = { .940, .932, .910 },
    textMuted = { .710, .705, .690 },
    textDim = { .590, .580, .590 },
    accent = { .835, .235, .285 },
    accentHover = { .950, .350, .400 },
    tooltipAccent = { .780, .460, .500 },
    disabled = { .400, .400, .420 },
    success = { .310, .690, .455 },
    warning = { .880, .645, .250 },
    danger = { .895, .300, .315 },
    font = STANDARD_TEXT_FONT or "Fonts\\FRIZQT__.TTF",
    spacing = { 6, 10, 16, 24 },
    iconHit = 32,
    roundedPanelTexture = "Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\RoundedPanel.png",
}

local readableSize = { [11] = 13, [12] = 14, [14] = 16, [16] = 18, [20] = 22 }
function ns.Theme.SetFont(region, size, color)
    region:SetFont(ns.Theme.font, readableSize[size] or size, "")
    region:SetShadowOffset(0, 0)
    region:SetTextColor(color[1], color[2], color[3], 1)
end

-- One self-owned 10-unit alpha corner mask for both workbench and receiver.
-- Flat stretches stay pixel-solid; only the four corner arcs use texture UVs.
function ns.Theme.CreateRoundedSurface(panel, surfaceColor, options)
    options = options or {}
    local radius, inset = options.radius or 12, options.inset or 0
    local regions = {}
    local function region(corner)
        local texture = panel:CreateTexture(nil, "BACKGROUND", nil, options.sublevel or 0)
        regions[#regions + 1] = { texture = texture, corner = corner }
        return texture
    end
    local middle = region()
    middle:SetPoint("TOPLEFT", panel, "TOPLEFT", inset + radius, -inset)
    middle:SetPoint("BOTTOMRIGHT", panel, "BOTTOMRIGHT", -inset - radius, inset)
    local left = region()
    left:SetPoint("TOPLEFT", panel, "TOPLEFT", inset, -inset - radius)
    left:SetPoint("BOTTOMLEFT", panel, "BOTTOMLEFT", inset, inset + radius)
    left:SetWidth(radius)
    local right = region()
    right:SetPoint("TOPRIGHT", panel, "TOPRIGHT", -inset, -inset - radius)
    right:SetPoint("BOTTOMRIGHT", panel, "BOTTOMRIGHT", -inset, inset + radius)
    right:SetWidth(radius)
    local corners = {
        {"TOPLEFT", 0, 0, 0, .375, 0, .375},
        {"TOPRIGHT", 0, 0, .625, 1, 0, .375},
        {"BOTTOMLEFT", 0, 0, 0, .375, .625, 1},
        {"BOTTOMRIGHT", 0, 0, .625, 1, .625, 1},
    }
    for _, corner in ipairs(corners) do
        local isRight = corner[1]:find("RIGHT", 1, true) ~= nil
        local isBottom = corner[1]:find("BOTTOM", 1, true) ~= nil
        local curved = not (options.squareRight and isRight)
        local texture = region(curved)
        if curved then
            texture:SetTexture(ns.Theme.roundedPanelTexture)
            texture:SetTexCoord(corner[4], corner[5], corner[6], corner[7])
        end
        texture:SetSize(radius, radius)
        texture:SetPoint(corner[1], panel, corner[1], isRight and -inset or inset, isBottom and inset or -inset)
    end
    local surface = { regions = regions, radius = radius, inset = inset }
    function surface:SetColor(color, alpha)
        self.color = color
        alpha = alpha or color[4] or 1
        for _, entry in ipairs(regions) do
            if entry.corner then entry.texture:SetVertexColor(color[1], color[2], color[3], alpha)
            else entry.texture:SetColorTexture(color[1], color[2], color[3], alpha) end
        end
    end
    surface:SetColor(surfaceColor or ns.Theme.window)
    return surface
end

function ns.Theme.PaintRoundedPanel(panel, surfaceColor, options)
    if not panel.lycheeSurface then
        panel.lycheeSurface = ns.Theme.CreateRoundedSurface(panel, surfaceColor, options)
    else panel.lycheeSurface:SetColor(surfaceColor or ns.Theme.window) end
    return panel.lycheeSurface
end

function ns.Theme.StopAnimation(frame)
    if frame and frame.lycheeMotionActive then
        frame.lycheeMotionActive = nil
        frame:SetScript("OnUpdate", nil)
        frame:SetAlpha(1)
    end
end

-- Lychee-style squash, rebound and settle, drawn only on our own logo.
-- The receiver owns the driver and its 20-second deadline; this sampler
-- allocates no frames, installs no hooks and never moves the optical symbol.
local connectionPoses = {
    { .16, 1.075, .925, 0 },
    { .50, .96, 1.045, 7 },
    { .84, 1.035, .965, 0 },
    { 1.10, .99, 1.012, 1 },
    { 1.40, 1, 1, 0 },
}
function ns.Theme.DrawConnectionBounce(logo, anchor, size, elapsed)
    local time = elapsed % 1.8
    local at, x, y, offset = 0, 1, 1, 0
    for _, pose in ipairs(connectionPoses) do
        if time <= pose[1] then
            local t = (time - at) / (pose[1] - at)
            local eased = t * t * (3 - 2 * t)
            x, y, offset = x + (pose[2] - x) * eased,
                y + (pose[3] - y) * eased, offset + (pose[4] - offset) * eased
            break
        end
        at, x, y, offset = pose[1], pose[2], pose[3], pose[4]
    end
    logo:SetSize(size * x, size * y)
    logo:SetPoint("CENTER", anchor, "CENTER", 0, size / 128 * (offset + 39 * (y - 1)))
end

-- One bounded driver per active surface. Hiding or replacing a page cancels it
-- synchronously; no idle update loop survives the 140 ms reveal.
function ns.Theme.Reveal(frame)
    if not frame then return end
    ns.Theme.StopAnimation(frame)
    local state=ns.Persistence and ns.Persistence.Current()
    if state and state.options.reducedMotion~=nil
        and not (issecretvalue and issecretvalue(state.options.reducedMotion)) then
        ns.Theme.reducedMotion=state.options.reducedMotion==true
    end
    if ns.Theme.reducedMotion then frame:SetAlpha(1); return end
    local elapsed = 0
    frame:SetAlpha(.01)
    frame.lycheeMotionActive = true
    frame:SetScript("OnUpdate", function(self, delta)
        elapsed = math.min(.14, elapsed + (tonumber(delta) or 0))
        local progress = elapsed / .14
        self:SetAlpha(1 - (1 - progress) * (1 - progress) * (1 - progress))
        if elapsed >= .14 then ns.Theme.StopAnimation(self) end
    end)
end
