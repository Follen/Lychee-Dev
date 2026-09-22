APIDocumentation:AddDocumentationTable({
	Name = "C_AuctionHouse",
	Type = "System",
	Tables = {
		{
			Name = "ItemSearchResultInfo",
			Type = "Structure",
			Fields = {
				{Name = "itemID", Type = "number", Nilable = false},
				{Name = "price", Type = "number", Nilable = false},
			},
		},
	},
	Functions = {
		{
			Name = "GetItemSearchResultInfo",
			Type = "Function",
			IsProtectedFunction = true,
			SecretArguments = "AllowedWhenUntainted",
			SecretWhenUnitSpellCastRestricted = true,
			Arguments = {
				{Name = "itemKey", Type = "ItemKey", Nilable = false},
				{Name = "sorts", Type = "table", Nilable = true},
			},
			Returns = {
				{Name = "itemSearchResultInfo", Type = "ItemSearchResultInfo", Nilable = true},
			},
		},
		{
			Name = "GetRestrictedInfo",
			Type = "Function",
			SecretWrapperConstant = "ContextuallySecret",
			SecretArgumentsAddAspect = {"UnitTokenRestrictedForAddOns"},
			SecretReturnsForAspect = {"UnitTokenPvPRestrictedForAddOns"},
			IsPreventingSecretValues = true,
			RestrictedTypes = {
				"UnitTokenRestrictedForAddOns",
				"UnitTokenPvPRestrictedForAddOns",
			},
			Arguments = {
				{Name = "unitToken", Type = "UnitToken", Nilable = false},
			},
			Returns = {
				{Name = "target", Type = "UnitToken", Nilable = true, ConditionalSecret = true},
				{Name = "castBarID", Type = "number", Nilable = false, NeverSecret = true},
			},
		},
		{
			Name = "StartCommoditiesPurchase",
			Type = "Function",
			Arguments = {
				{Name = "itemID", Type = "number", Nilable = false},
				{Name = "quantity", Type = "number", Nilable = false},
			},
		},
		{
			Name = "CancelCommoditiesPurchase",
			Type = "Function",
			Arguments = {
				{Name = "itemID", Type = "number", Nilable = false},
			},
			Returns = nil,
		},
	},
	Events = {
		{
			Name = "AUCTION_HOUSE_SHOW",
			Type = "Event",
			Payload = {
				{Name = "auctionHouseID", Type = "number", Nilable = false},
				{Name = "isAethereal", Type = "bool", Nilable = false},
			},
		},
	},
	Widgets = {
		{
			Name = "Button",
			Type = "Widget",
			Methods = {
				{
					Name = "SetText",
					Type = "ScriptObject",
					Arguments = {
						{Name = "text", Type = "string", Nilable = false},
						{Name = "isFormatted", Type = "bool", Nilable = true},
					},
				},
			},
		},
	},
})
