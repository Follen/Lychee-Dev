local flag = ...
local function Choose()
    if flag then return Source() else return Pair() end
end
Use(Choose())
