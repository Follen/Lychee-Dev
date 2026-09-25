package selection

import (
	"strings"
	"testing"
)

func TestDataIdentityCanonicalProducts(t *testing.T) {
	want := map[string]string{
		"retail":  "wow",
		"classic": "wow_classic",
		"titan":   "wow_classic_titan",
		"forever": "wow_forever",
	}
	for product, installationProduct := range want {
		t.Run(product, func(t *testing.T) {
			pin := validDataIdentityPin()
			pin.Product = product
			gotProduct, gotLocale, err := DataIdentity(pin)
			if err != nil {
				t.Fatal(err)
			}
			if gotProduct != installationProduct || gotLocale != 0x2 {
				t.Fatalf("got product=%q locale=%#x, want product=%q locale=%#x", gotProduct, gotLocale, installationProduct, uint32(0x2))
			}
		})
	}
}

func TestDataIdentityLocaleFlags(t *testing.T) {
	want := map[string]uint32{
		"enUS": 0x2,
		"koKR": 0x4,
		"frFR": 0x10,
		"deDE": 0x20,
		"zhCN": 0x40,
		"esES": 0x80,
		"zhTW": 0x100,
		"enGB": 0x200,
		"esMX": 0x1000,
		"ruRU": 0x2000,
		"ptBR": 0x4000,
		"itIT": 0x8000,
		"ptPT": 0x10000,
	}
	for language, wantLocale := range want {
		t.Run(language, func(t *testing.T) {
			pin := validDataIdentityPin()
			pin.Language = language
			product, locale, err := DataIdentity(pin)
			if err != nil {
				t.Fatal(err)
			}
			if product != "wow" || locale != wantLocale {
				t.Fatalf("got product=%q locale=%#x, want product=%q locale=%#x", product, locale, "wow", wantLocale)
			}
		})
	}
}

func TestDataIdentityRejectsMalformedUnresolvedAndUnsupportedPins(t *testing.T) {
	malformedBuild := validDataIdentityPin()
	malformedBuild.FullBuild = "12.1.0"
	badDigest := validDataIdentityPin()
	badDigest.BuildConfig = strings.Repeat("g", 32)

	tests := []struct {
		name string
		pin  DataPin
		want string
	}{
		{name: "zero pin", pin: DataPin{}, want: "selection.unresolved_data"},
		{name: "malformed build", pin: malformedBuild, want: "selection.unresolved_data"},
		{name: "invalid digest", pin: badDigest, want: "selection.unresolved_data"},
		{name: "unsupported product", pin: func() DataPin { p := validDataIdentityPin(); p.Product = "ptr"; return p }(), want: "selection.unsupported_data_product"},
		{name: "unsupported language", pin: func() DataPin { p := validDataIdentityPin(); p.Language = "jaJP"; return p }(), want: "selection.unsupported_data_language"},
		{name: "unsupported region", pin: func() DataPin { p := validDataIdentityPin(); p.Region = "xx"; return p }(), want: "selection.unsupported_data_region"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if _, _, err := DataIdentity(test.pin); err == nil || err.Error() != test.want {
				t.Fatalf("error %v, want %s", err, test.want)
			}
		})
	}
}

func validDataIdentityPin() DataPin {
	return DataPin{
		Product:          "retail",
		Region:           "cn",
		FullBuild:        "12.1.0.69875",
		BuildConfig:      strings.Repeat("a", 32),
		CDNConfig:        strings.Repeat("b", 32),
		Language:         "enUS",
		DefinitionCommit: strings.Repeat("c", 40),
	}
}

// A product ID names a reusable manifest slot, not a fixed game: the Forever
// install flavor (wow_forever) differs from the slot carrying its data
// (wow_classic_beta). URL construction must use the slot.
func TestDataProductSlotDiffersFromInstallFlavor(t *testing.T) {
	want := map[string]string{
		"retail":  "wow",
		"classic": "wow_classic",
		"titan":   "wow_classic_titan",
		"forever": "wow_classic_beta",
	}
	for product, slot := range want {
		got, err := DataProductSlot(product)
		if err != nil || got != slot {
			t.Fatalf("%s: slot=%q err=%v, want %q", product, got, err, slot)
		}
	}
	if _, err := DataProductSlot("nope"); err == nil {
		t.Fatal("unknown product accepted")
	}
	// The installation flavor code stays wow_forever for install identity.
	got, err := DataProduct("forever")
	if err != nil || got != "wow_forever" {
		t.Fatalf("install flavor changed: %q %v", got, err)
	}
}
