local function compute(...) return select('#', ...) end
local alias = compute
return alias(1, 2)
