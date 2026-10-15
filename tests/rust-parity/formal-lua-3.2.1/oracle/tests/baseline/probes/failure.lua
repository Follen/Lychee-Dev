local probe = ...
local ns = LycheeDevInternal
assert(not ns.InputProtection.IsActive(), "input_protected_during_execution")
error("BASELINE_EXPECTED_FAILURE", 0)
