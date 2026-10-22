local x = Source()
local f = function() return x end
x = nil
Use(f())
