local first, second, third = ...
local observedFirst = first
local observedSecond = second
local observedThird = third
local shiftedFirst, shiftedSecond, shiftedThird = 7, ...
local observedShiftedFirst = shiftedFirst
local observedShiftedSecond = shiftedSecond
local observedShiftedThird = shiftedThird
local assignedFirst, assignedSecond, assignedThird = 1, 2, 3
assignedFirst, assignedSecond, assignedThird = ...
local observedAssignedFirst = assignedFirst
local observedAssignedSecond = assignedSecond
local observedAssignedThird = assignedThird
function ReturnTail(...)
    return ...
end
function CallTail(...)
    return Missing(...)
end
