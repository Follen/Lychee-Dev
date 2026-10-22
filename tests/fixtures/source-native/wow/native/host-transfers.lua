local object = { field = Source() }
local copied = object.field
object.field = nil
Use(copied)
Use(object.field)
local function Factory(value)
    return function() return value end
end
local a = Factory(Source())
local b = Factory(Pair())
Use(a())
Use(b())
