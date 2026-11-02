local object = {}
local key = "missing"

---@return number
function WrongFixedReturn()
    return Replacement(object[key])
end
