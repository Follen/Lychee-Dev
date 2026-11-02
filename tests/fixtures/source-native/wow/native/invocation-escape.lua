local t = { value = 1 }
local untouched = { value = 7 }
local unknown = ...
unknown(t)
t.value = "safe"
unknown()
local afterEscaped = t.value
local afterUntouched = untouched.value
