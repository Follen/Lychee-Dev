-- R3-06: the real picker must stop on combat even after hiding its window.
local Env = ...
local ns = Env.LoadWorkbench()
Env.LoadAddon("Modules/ObjectInspector.lua", ns)
Env.LoadAddon("UI/Pages/Object.lua", ns)
assert(ns.Persistence.Load())
assert(Env.framesCreated == 0 and #Env.pendingTimers == 0 and not LycheeToolkitPickerDock,
    "unused Object page created runtime work")

assert(ns.Workbench.ShowPage("objects"))
local page = assert(ns.Workbench.GetPage("objects"))
assert(not LycheeToolkitPickerDock, "Object page eagerly created its unused picker")
page.mouseButton:Click()
local dock = assert(LycheeToolkitPickerDock)
assert(not ns.Workbench.IsShown() and dock:IsShown() and dock:GetScript("OnKeyDown"),
    "real picker did not hide the workbench and acquire its keyboard handler")
Env.SetInCombat(true)
ns.Safety.RunCombatShutdown()
assert(not ns.Workbench.IsShown() and not dock:IsShown() and not dock:GetScript("OnKeyDown"),
    "combat retained the picker or its keyboard handler while the window was hidden")
for _, key in ipairs({ "F", "ENTER", "ESCAPE" }) do
    dock.propagateKeyboardInput = true
    Env.FireScript(dock, "OnKeyDown", key)
    assert(dock.propagateKeyboardInput == true and not ns.Workbench.IsShown(),
        "combat picker swallowed a key or reopened the window")
end

-- Reusing the picker must not allocate another frame/driver, and normal
-- capture/cancellation still restore the real window outside combat.
Env.SetInCombat(false)
assert(ns.Workbench.ShowPage("objects"))
local frames = Env.framesCreated
page.mouseButton:Click()
assert(LycheeToolkitPickerDock == dock and Env.framesCreated == frames and dock:IsShown())
Env.FireScript(dock, "OnKeyDown", "ESCAPE")
assert(ns.Workbench.IsShown() and not dock:IsShown() and not dock:GetScript("OnKeyDown"))
local target = CreateFrame("Frame", "PickerCombatTarget", UIParent)
Env.mouseFoci = { target }
page.mouseButton:Click()
Env.FireScript(dock, "OnKeyDown", "F")
assert(ns.Workbench.IsShown() and not dock:IsShown() and page:GetSelection() == target,
    "picker failed to capture normally after combat")
page.mouseButton:Click()
Env.SetInCombat(true)
ns.Safety.RunCombatShutdown()
assert(not dock:IsShown() and not dock:GetScript("OnKeyDown"), "restarted picker survived combat")
print("Object picker hidden-window combat shutdown ok")
