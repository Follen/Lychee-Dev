package selection

import (
	"errors"
	"fmt"
	"strings"
)

var ErrDataProductBuild = errors.New("selection.data_product_build_mismatch")

// ValidateDataRelease admits current targets and guards the known Forever series.
func ValidateDataRelease(product, fullBuild string) error {
	if profile, ok := dataProductProfile(product); ok {
		if product == "forever" && (!build(fullBuild) || !strings.HasPrefix(fullBuild, profile.BuildSeries+".")) {
			return fmt.Errorf("%w: %s slot %s serves %s; verified series is %s", ErrDataProductBuild, product, profile.ProductCode, fullBuild, profile.BuildSeries)
		}
		return nil
	}
	return errors.New("selection.unsupported_data_product")
}

// DataIdentity validates an exact data pin and translates the toolkit product
// into the installation's product code. Region remains an explicit provenance
// choice; local archives do not independently attest where they were downloaded.
func DataIdentity(pin DataPin) (product string, locale uint32, err error) {
	if err = validatePins(SelectionSpec{Data: &pin}); err != nil {
		return
	}
	if pin.ProductCode != "" {
		product, err = DataProduct(pin.Product)
		if err != nil {
			return
		}
		if pin.ProductCode != product {
			return "", 0, errors.New("selection.data_product_code_mismatch")
		}
	} else {
		switch pin.Product {
		case "forever":
			product = "wow_forever"
		case "forever-asia-beta":
			product = "wow_classic_beta"
		case "forever-cn-beta":
			product = "wow_cn_beta"
		default:
			product, err = DataProduct(pin.Product)
		}
		if err != nil {
			return
		}
	}
	if SourceProductFamily(pin.Product) == "forever" && !strings.HasPrefix(pin.FullBuild, "1.60.1.") {
		return "", 0, ErrDataProductBuild
	}
	locale, err = DataLocale(pin.Region, pin.Language)
	return
}

// DataProductSlot names the current remote catalog. Historical pins use
// PinnedDataProductSlot instead.
func DataProductSlot(product string) (string, error) { return DataProduct(product) }

// DataProduct translates a toolkit track without deriving identity from a path.
func DataProduct(product string) (string, error) {
	if profile, ok := dataProductProfile(product); ok {
		return profile.ProductCode, nil
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

// PinnedDataProductSlot preserves the endpoint recorded by an immutable pin.
func PinnedDataProductSlot(pin DataPin) (string, error) {
	product, _, err := DataIdentity(pin)
	if err != nil {
		return "", err
	}
	if product == "wow_forever" {
		return "wow_classic_beta", nil
	}
	return product, nil
}

// ValidateNewSelection applies current target admission without rewriting historical pins.
func ValidateNewSelection(spec SelectionSpec) error {
	if spec.Data != nil {
		product, err := DataProduct(spec.Data.Product)
		if err != nil {
			return err
		}
		if spec.Data.Product == "forever" && spec.Data.ProductCode != product {
			return errors.New("selection.formal_forever_requires_wowf_identity")
		}
		if spec.Data.ProductCode != "" && spec.Data.ProductCode != product {
			return errors.New("selection.data_product_code_mismatch")
		}
		if _, _, err := DataIdentity(*spec.Data); err != nil {
			return err
		}
	}
	if spec.Changes != nil {
		if err := ValidateDataRelease(spec.Changes.Product, spec.Changes.FullBuild); err != nil {
			return err
		}
	}
	return validatePins(spec)
}

// NewDataProductCode records the product discriminator where legacy pins are ambiguous.
func NewDataProductCode(product string) string {
	if product == "forever" {
		return "wowf"
	}
	return ""
}
