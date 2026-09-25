package selection

import "errors"

// DataIdentity validates an exact data pin and translates the toolkit product
// into the installation's product code. Region remains an explicit provenance
// choice; local archives do not independently attest where they were downloaded.
func DataIdentity(pin DataPin) (product string, locale uint32, err error) {
	if err = validatePins(SelectionSpec{Data: &pin}); err != nil {
		return
	}
	product, err = DataProduct(pin.Product)
	if err != nil {
		return "", 0, err
	}
	locale, err = DataLocale(pin.Region, pin.Language)
	return
}

// DataProductSlot returns the versions-manifest slot a track's data is
// published under. The installation flavor code and the data slot differ for
// tracks that reuse another product's slot (Forever on wow_classic_beta).
func DataProductSlot(product string) (string, error) {
	for _, baseline := range VerifiedClientBaselines() {
		if baseline.Product == product {
			if baseline.DataSlot != "" {
				return baseline.DataSlot, nil
			}
			return baseline.ProductCode, nil
		}
	}
	return "", errors.New("selection.unsupported_data_product")
}

// DataProduct translates a toolkit track without deriving identity from a path.
func DataProduct(product string) (string, error) {
	for _, baseline := range VerifiedClientBaselines() {
		if baseline.Product == product {
			return baseline.ProductCode, nil
		}
	}
	return "", errors.New("selection.unsupported_data_product")
}

// DataLocale validates a requested region/language before any preparation I/O.
// The region is a declared choice, not a fact inferred from local archive bytes.
func DataLocale(region, language string) (locale uint32, err error) {
	switch language {
	case "enUS":
		locale = 0x2
	case "koKR":
		locale = 0x4
	case "frFR":
		locale = 0x10
	case "deDE":
		locale = 0x20
	case "zhCN":
		locale = 0x40
	case "esES":
		locale = 0x80
	case "zhTW":
		locale = 0x100
	case "enGB":
		locale = 0x200
	case "esMX":
		locale = 0x1000
	case "ruRU":
		locale = 0x2000
	case "ptBR":
		locale = 0x4000
	case "itIT":
		locale = 0x8000
	case "ptPT":
		locale = 0x10000
	default:
		return 0, errors.New("selection.unsupported_data_language")
	}
	switch region {
	case "us", "eu", "cn", "kr", "tw":
	default:
		return 0, errors.New("selection.unsupported_data_region")
	}
	return
}
