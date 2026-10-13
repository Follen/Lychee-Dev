local before = Later

---@return string
function Deferred()
    return Later("ok")
end

---@return number
function WrongDeferredReturn()
    return Later("ok")
end

return before
