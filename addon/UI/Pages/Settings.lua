local ADDON_NAME, ns = ...

local W, Theme = ns.Widgets, ns.Theme
local actions={"wake","submit","close"}
local titles={wake="RECEIVER_BINDING_WAKE",submit="RECEIVER_BINDING_SUBMIT",close="RECEIVER_BINDING_CLOSE"}
local function display(chord)
    return (chord or ""):gsub("ALT%-CTRL%-SHIFT%-","Ctrl+Alt+Shift+"):gsub("ALT%-CTRL%-","Ctrl+Alt+")
end
local function label(parent,text,size,color)
    local value=parent:CreateFontString(nil,"OVERLAY","GameFontHighlightSmall")
    Theme.SetFont(value,size,color or Theme.text)
    value:SetText(text);value:SetJustifyH("LEFT")
    return value
end

ns.SettingsPage={Create=function(parent,width)
    local page=CreateFrame("Frame",nil,parent)
    page:SetAllPoints(parent)
    local scroll=W.CreateScrollArea(page,24,50,16,12)
    local column=CreateFrame("Frame",nil,scroll)
    column:SetSize(540,490)
    scroll:SetScrollChild(column)
    page.column,page.scroll,page.bindings=column,scroll,{}
    local heading=label(column,ns.L.SETTINGS,20)
    heading:SetPoint("TOPLEFT",0,0)
    local introduction=label(column,ns.L.SETTINGS_HELP,12,Theme.textMuted)
    introduction:SetPoint("TOPLEFT",0,-36);introduction:SetWordWrap(true)
    local appearance=label(column,ns.L.SETTINGS_APPEARANCE,14)
    appearance:SetPoint("TOPLEFT",0,-91)
    local motionLabel=label(column,ns.L.REDUCE_MOTION,12)
    motionLabel:SetPoint("TOPLEFT",0,-128)
    local motion=W.CreateToggle(column,function(checked)
        Theme.reducedMotion=checked
        local state=ns.Persistence.Current()
        if state then state.options.reducedMotion=checked end
        if checked then Theme.StopAnimation(page) end
        if ns.ActivityView then ns.ActivityView.Refresh() end
    end)
    local persisted=ns.Persistence.Current()
    if persisted and persisted.options.reducedMotion~=nil then Theme.reducedMotion=persisted.options.reducedMotion==true end
    motion:SetChecked(Theme.reducedMotion, true)
    page.motionToggle=motion
    local section=label(column,ns.L.RECEIVER_BINDINGS,14)
    section:SetPoint("TOPLEFT",0,-181)
    local help=label(column,ns.L.RECEIVER_BINDING_FIXED_HELP,12,Theme.textMuted)
    help:SetPoint("TOPLEFT",0,-210);help:SetWordWrap(true)
    for _,action in ipairs(actions) do
        local name=label(column,ns.L[titles[action]],13)
        local button=W.CreateShortcutButton(column)
        button:EnableMouse(false)
        button:SetScript("OnEnter",nil)
        button:SetScript("OnLeave",nil)
        page.bindings[action]={button=button,label=name}
    end
    page.RefreshBindingFields=function()
        local profile=ns.ReceiverBindings and ns.ReceiverBindings.Current()
        for _,action in ipairs(actions) do
            page.bindings[action].button:SetShortcut(display(profile and profile[action]),false)
        end
    end
    page.FitContent=function()
        local available=math.max(210,width()-48)
        local compact=available<450
        column:SetWidth(math.min(600,available))
        column:SetHeight(compact and 554 or 428)
        introduction:SetWidth(column:GetWidth())
        help:SetWidth(column:GetWidth())
        motion:ClearAllPoints();motion:SetPoint("TOPRIGHT",column,"TOPRIGHT",-4,-124)
        for index,action in ipairs(actions) do
            local control=page.bindings[action]
            local y=-265-(index-1)*(compact and 84 or 48)
            control.label:ClearAllPoints();control.label:SetPoint("TOPLEFT",0,y)
            control.button:ClearAllPoints()
            control.button:SetWidth(compact and column:GetWidth() or 220)
            control.button:SetPoint("TOPRIGHT",column,"TOPRIGHT",0,compact and y-24 or y+9)
        end
        scroll:UpdateScrollChildRect()
    end
    page:SetScript("OnSizeChanged",page.FitContent)
    page.RefreshBindingFields();page.FitContent()
    return page
end}
