local hidden = { value = 1 }
local function Update()
    hidden.value = "changed"
end
local unknown = ...
unknown(Update)
hidden.value = "safe"
unknown()
local afterEscapedClosure = hidden.value
