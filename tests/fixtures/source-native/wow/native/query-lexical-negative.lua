local value=1
local function f(value)
 -- value()
 local s="value()"
 for value=1,2 do local z=value end
 return value
end
local value=value
return f,value
