-- Same field geometry and focus feedback for source, search and copyable output.
local Env = ...
local ns = Env.LoadWorkbench()
local W, Theme = ns.Widgets, ns.Theme
assert(Env.framesCreated == 0)
local fields = { W.CreateLineInput(UIParent), W.CreateTextArea(UIParent, false), W.CreateTextArea(UIParent, true) }
for _, field in ipairs(fields) do
    assert(field.fieldOutline.radius == Theme.controlRadius)
    assert(field.lycheeSurface.radius == Theme.controlRadius - 1 and field.lycheeSurface.inset == 1)
    assert(not field.backdropInfo, "field regained a square backdrop")
    local created = Env.framesCreated
    Env.FireScript(field.editBox, "OnEditFocusGained")
    assert(field.fieldOutline.color == Theme.accent)
    Env.FireScript(field.editBox, "OnEditFocusLost")
    assert(field.fieldOutline.color == Theme.field, "resting field retained a permanent outline")
    assert(Env.framesCreated == created, "focus allocated new controls")
end
local input, output = fields[2], fields[3]
input:SetPlaceholder("Enter Lua")
assert(input.placeholder:IsShown())
input.editBox:SetText("return 42")
assert(not input.placeholder:IsShown())
input.editBox:SetText("")
assert(input.placeholder:IsShown())
output:SetPlaceholder("Result")
W.SetReadOnlyText(output, "original result")
output.editBox:SetText("modified")
assert(output.editBox:GetText() == "original result" and not output.placeholder:IsShown())
local mode = W.CreateModeTab(UIParent, "Text")
mode:SetActive(true)
assert(mode.active and mode.variant == "selected" and not mode.underline,
    "view switch retained a sidebar marker")
W.SetButtonEnabled(mode, false)
assert(not mode:IsEnabled())
local tree = ns.TreeView.Create(UIParent)
assert(tree.panel.fieldOutline.radius == input.fieldOutline.radius)
assert(tree.panel.fieldOutline.color == input.fieldOutline.color,
    "tree and text mode do not share their resting surface")
local shortcut = W.CreateShortcutButton(UIParent)
shortcut:SetShortcut("Ctrl+Alt+Shift+F12", false)
assert(not shortcut.label:IsShown() and shortcut.keycaps[4].label:GetText() == "F12")
shortcut:SetShortcut("Press keys", true)
assert(shortcut.label:IsShown() and not shortcut.keycaps[1]:GetParent():IsShown())
shortcut:SetShortcut("Ctrl+Alt+[", false)
assert(not shortcut.keycaps[4]:IsShown() and shortcut.keycaps[3].label:GetText() == "[")

local toggle = W.CreateToggle(UIParent)
local created = Env.framesCreated
toggle:Click()
assert(toggle.checked and toggle:GetScript("OnUpdate"), "switch did not start its bounded slide")
Env.FireScript(toggle, "OnUpdate", .2)
assert(not toggle:GetScript("OnUpdate") and toggle.thumb.point[4] == 16)
toggle:Click()
Env.FireScript(toggle, "OnHide")
assert(not toggle:GetScript("OnUpdate") and toggle.thumb.point[4] == 2,
    "switch retained motion or an intermediate position after hide")
Theme.reducedMotion = true
for index = 1, 2000 do toggle:Click() end
assert(not toggle:GetScript("OnUpdate") and Env.framesCreated == created,
    "switch leaked controls or ignored reduced motion")
local logo = UIParent:CreateTexture()
Theme.DrawConnectionBounce(logo, UIParent, 96, 0)
assert(logo:GetWidth() == 96 and logo:GetHeight() == 96)
Theme.DrawConnectionBounce(logo, UIParent, 96, .16)
assert(logo:GetWidth() > 96 and logo:GetHeight() < 96, "brand did not squash before rebounding")
Theme.DrawConnectionBounce(logo, UIParent, 96, .5)
assert(logo.point[5] > 0 and logo:GetHeight() > 96, "brand rebound missing")
Theme.DrawConnectionBounce(logo, UIParent, 96, 1.6)
assert(logo:GetWidth() == 96 and logo:GetHeight() == 96, "brand did not settle between cycles")
print("shared fields ok")
