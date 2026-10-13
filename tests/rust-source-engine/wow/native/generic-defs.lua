---@generic T
---@param value T
---@return T
function Identity(value)
    return value
end

---@param value string
---@return string
function Replacement(value)
    return value
end

Methods = {}
---@generic T
---@param label string
---@param value T
---@return T
function Methods:Echo(label, value)
    return value
end
