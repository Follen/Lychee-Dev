local object = {}
object.run = F1
local key = "run"
local dynamic = object[key]
setmetatable(object, {})
local uncertain = object.run
local conditional = {}
conditional.run = F1
if Flag then
    conditional.run = F2
end
local branchValue = conditional.run
return dynamic, uncertain, branchValue
