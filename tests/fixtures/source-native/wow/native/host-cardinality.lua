local flag = ...
local function Variable()
    if flag then return Use() else return Pair() end
end
UseSecond(Variable())
UseSecond((Variable()))
