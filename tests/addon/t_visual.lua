local Env = ...
local ns = Env.LoadWorkbench()
assert(ns.Persistence.Load(), "persistence did not load")
local W = ns.Workbench
local theme = ns.Theme
local profile = { wake = "ALT-CTRL-]", submit = "ALT-CTRL-SHIFT-]", close = "ALT-CTRL-[" }
ns.ReceiverBindings = {
    Current = function() return profile end,
    Display = function(action) return profile[action] end,
    Configure = function(action, chord)
        if chord == profile.close and action ~= "close" then
            return nil, "receiver_binding_conflict"
        end
        profile[action] = chord
        return profile
    end,
    ConfigureProfile = function(proposed)
        if proposed.wake==proposed.close or proposed.submit==proposed.close then return nil,"receiver_binding_conflict" end
        for key,value in pairs(proposed) do profile[key]=value end
        return profile
    end,
    Reset = function()
        profile.wake = "ALT-CTRL-]"
        profile.submit = "ALT-CTRL-SHIFT-]"
        profile.close = "ALT-CTRL-["
        return profile
    end,
}
assert(Env.framesCreated == 0, "theme or workbench built frames while disabled")
assert(theme.window[1] == .055 and theme.text[1] == .940
    and theme.accent[1] == .835, "workbench tokens differ from Design.md")

assert(W.Toggle(), "workbench did not open")
local frame = LycheeToolkitWindow
assert(frame:GetWidth() == 960 and frame:GetHeight() == 660,
    "workbench dimensions changed")
local function corners(surface)
    local count = 0
    for _, region in ipairs(surface.regions) do
        if region.corner then count = count + 1 end
    end
    return count
end
assert(corners(frame.lycheeSurface) == 4 and corners(frame.sidebar.lycheeSurface) == 2,
    "main window and flush sidebar did not share the outside contour")
assert(frame.sidebar.lycheeSurface.inset == 0, "sidebar inset left a strip at the window edge")
assert(frame.pageViewport.point[1] == "BOTTOMRIGHT" and frame.pageViewport.point[3] == 0,
    "page viewport clips the page-owned footer")
assert(not frame.settingsButton and not W.ShowSettings and not ns.SettingsPage,
    "removed settings surface returned")
assert(frame.navScroll and frame.pageTabs.runner.label.fontHeight == 14,
    "primary navigation did not use the readable type scale")
assert(W.GetSecondaryRoot() and not W.GetSecondaryRoot():IsShown(),
    "secondary body started visible")

local secondary=CreateFrame("Frame",nil,W.GetSecondaryRoot())
assert(W.ShowSecondary(secondary))
assert(W.GetSecondaryRoot():IsShown() and not frame.pageTabs.runner.active,
    "secondary content did not replace the body")
assert(W.HideSecondary() and not W.GetSecondaryRoot():IsShown(),
    "back did not restore the original page")
assert(W.GetActivePage() == "runner" and frame.pageTabs.runner.active,
    "back lost the active primary page")
assert(frame.backButton and frame.backButton.label:GetText() == ns.L.BACK,
    "secondary navigation did not name the return action")

local previous = W.GetActivePage()
for _, key in ipairs({"objects", "events", "trace", "diagnostics",
    "exports", "automation", "about", "runner"}) do
    -- Unregistered pages still have a shell tab; this checks navigation state
    -- without requiring unrelated business module fixtures in this suite.
    frame.pageTabs[key]:Click()
    assert(W.GetActivePage() == key, "page selection failed: " .. key)
end
assert(previous == "runner" and Env.framesCreated > 0,
    "workbench did not reuse its lazy shell")

W.Close()
assert(not frame:IsShown(), "workbench did not hide")
assert(not frame.lycheeMotionActive, "window animation survived hide")
UIParent:SetSize(700, 600)
assert(W.Open(), "workbench did not reopen at a smaller viewport")
assert(frame:GetWidth() == 676 and frame:GetHeight() == 576,
    "workbench did not fit inside the viewport")
assert(frame.navScroll.verticalRange >= 0,
    "sidebar navigation has an invalid range")
assert(frame.pagePrevious:IsShown() and frame.pageNext:IsShown(),
    "narrow pages had no horizontal access controls")
assert(frame.pagePrevious.point[1] == "TOPRIGHT", "overflow controls occupy the page footer")
frame.pageNext:Click()
assert(frame.pageViewport:GetHorizontalScroll() > 0,
    "page body could not scroll horizontally")
assert(W.ShowPage("about"), "about destination is inaccessible")
W.Close()
UIParent:SetSize(500, 480)
assert(W.Open())
assert(frame:GetWidth() == 476 and frame:GetHeight() == 456)
assert(W.ShowPage("runner") and W.ShowPage("about") and frame.navScroll:GetVerticalScroll()>0,
    "small viewport did not scroll to the destination")
W.Close()
print("visual shell ok")
