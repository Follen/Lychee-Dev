package selection

// DataProductProfile identifies a delivery track independently of the Lua API
// family. In particular, the two Forever beta products can have the same build
// number and different build configurations.
type DataProductProfile struct {
	Product     string `json:"product"`
	Family      string `json:"family"`
	ProductCode string `json:"productCode"`
	Folder      string `json:"folder"`
	BuildSeries string `json:"buildSeries"`
	ReusedSlot  bool   `json:"reusedSlot,omitempty"`
}

func DataProducts() []DataProductProfile {
	return []DataProductProfile{
		{Product: "retail", Family: "retail", ProductCode: "wow", Folder: "_retail_", BuildSeries: "12.1.0"},
		{Product: "classic", Family: "classic", ProductCode: "wow_classic", Folder: "_classic_", BuildSeries: "5.5.4"},
		{Product: "classic-era", Family: "classic-era", ProductCode: "wow_classic_era", Folder: "_classic_era_", BuildSeries: "1.15.9"},
		{Product: "titan", Family: "titan", ProductCode: "wow_classic_titan", Folder: "_classic_titan_", BuildSeries: "3.80.2"},
		{Product: "forever-asia-beta", Family: "forever", ProductCode: "wow_classic_beta", Folder: "_classic_beta_", BuildSeries: "1.60.1", ReusedSlot: true},
		{Product: "forever-cn-beta", Family: "forever", ProductCode: "wow_cn_beta", Folder: "_cn_beta_", BuildSeries: "1.60.1", ReusedSlot: true},
		{Product: "anniversary", Family: "anniversary", ProductCode: "wow_anniversary", Folder: "_anniversary_", BuildSeries: "2.5.6"},
	}
}

func dataProductProfile(product string) (DataProductProfile, bool) {
	for _, profile := range DataProducts() {
		if profile.Product == product {
			return profile, true
		}
	}
	return DataProductProfile{}, false
}

// SourceProductFamily names the UI/API family, independently of the regional
// data delivery track. Both Forever beta products use the upstream forever ref.
func SourceProductFamily(product string) string {
	if profile, ok := dataProductProfile(product); ok {
		return profile.Family
	}
	return product
}

// SourceSupportsProduct also accepts an existing family-wide Blizzard source
// pin. A regional source pin does not authorize a different regional data pin.
func SourceSupportsProduct(source SourcePin, product string) bool {
	return source.Product == product || source.Repository == "wow-ui-source" && source.Product == SourceProductFamily(product)
}

// ClientDataProduct uses observed launcher metadata, never the directory name,
// to distinguish delivery tracks. Old wow_forever-only installations retain
// their legacy identity because their metadata does not identify a beta slot.
func ClientDataProduct(client ClientInstallation) string {
	for _, profile := range DataProducts() {
		if client.CatalogProduct == profile.ProductCode || client.ProductCode == profile.ProductCode {
			return profile.Product
		}
	}
	return client.Product
}

func ClientMatchesProduct(client ClientInstallation, product string) bool {
	return product == "" || product == client.Product || product == ClientDataProduct(client)
}

func ClientFolders() []string {
	folders := []string{"_forever_"}
	for _, profile := range DataProducts() {
		folders = append(folders, profile.Folder)
	}
	return folders
}

// Installation discovery does not claim a verified addon/API profile. Keep the
// pinned source baselines separate: a data-only family has Interface zero until
// its runtime/API acceptance is recorded.
func clientIdentityBaselines() []ClientBaseline {
	baselines := VerifiedClientBaselines()
	for _, profile := range DataProducts() {
		known := false
		for _, baseline := range baselines {
			known = known || baseline.Product == profile.Family
		}
		if !known {
			baselines = append(baselines, ClientBaseline{Product: profile.Family, ProductCode: profile.ProductCode, BuildSeries: profile.BuildSeries, TOC: MainTOC})
		}
	}
	return baselines
}
