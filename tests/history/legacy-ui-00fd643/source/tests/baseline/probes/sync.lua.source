local probe = ...
local ns = LycheeDevInternal
assert(ns and ns.Startup.ready, "addon_not_ready")
assert(not ns.Receiver.IsActive(), "receiver_active_during_execution")
assert(ns.ReceiptView.Current() == nil, "optical_overlay_during_execution")
assert(ns.Persistence.Bridge() == LycheeToolkitBridgeDB, "wrong_report_scope")
local sum = 0
for value = 1, 10 do sum = sum + value end
return { passed = true, baseline = "LIVE-01", sum = sum }
