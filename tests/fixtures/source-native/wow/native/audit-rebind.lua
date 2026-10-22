Global = { value = 1 }
local Alias = Global
local function Rebind()
    Global = { value = "fresh" }
end
Rebind()
Global.value = true
local after = Alias.value
