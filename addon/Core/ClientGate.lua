local ADDON_NAME, ns = ...

-- The single authority for client differences. One TOC serves every supported
-- build through its multi-interface declaration; the profile below is derived
-- from the running client's own build observation, never from which TOC file
-- a particular engine chose to read. Keep every product-specific value in the
-- baselines table and every product-specific branch behind ns.Client.
ns.PlatformProfile = nil
ns.Client = nil

local baselines = {
	{ product = "forever", version = "1.60.1", interface = 16001 },
	{ product = "classic", version = "5.5.4", interface = 50504 },
	{ product = "titan", version = "3.80.2", interface = 38002 },
	{ product = "retail", version = "12.1.0", interface = 120100 },
}

-- Select strictly by the observed interface number. Unsupported builds stay
-- unselected: Runtime reports the failure instead of guessing a profile.
-- GetBuildInfo is guarded so catalog-only loads (hosted test harnesses without
-- engine stubs) simply leave the client unselected instead of erroring.
local interface = type(GetBuildInfo) == "function" and select(4, GetBuildInfo()) or nil
if type(interface) == "number" then
	for _, baseline in ipairs(baselines) do
		if interface == baseline.interface then
			ns.PlatformProfile = baseline
			ns.Client = baseline.product
			break
		end
	end
end
