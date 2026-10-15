local probe = ...
local ns = LycheeDevInternal
assert(not ns.Receiver.IsActive(), "receiver_active_during_execution")
assert(ns.ReceiptView.Current() == nil, "optical_overlay_during_execution")
error("BASELINE_EXPECTED_FAILURE", 0)
