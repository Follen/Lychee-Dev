local hidden = { value = 1 }
local exposed = {
    f = function()
        hidden.value = "changed"
    end,
}
local function Invoke(box)
    box.f()
end
Invoke(exposed)
local after = hidden.value
