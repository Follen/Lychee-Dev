package records

import (
	"bytes"
	"errors"
	"fmt"
	"strings"
	"testing"
)

const (
	remoteBuildKey  = "0123456789abcdef0123456789abcdef"
	remoteCDNKey    = "fedcba9876543210fedcba9876543210"
	remoteRegion    = "us"
	remoteFullBuild = "12.1.0.69875"
)

func TestParseReleaseCatalogSelectsExactBuild(t *testing.T) {
	raw := "\ufeff# generated catalog\r\n# seqn = 7\r\nRegion!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\r\n" +
		remoteRegion + "|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\r\n" +
		"eu|bad-key|also-bad|not-an-integer|not-a-build\r\n"

	got, err := parseReleaseCatalog([]byte(raw), remoteRegion, remoteFullBuild)
	if err != nil {
		t.Fatalf("parseReleaseCatalog() error = %v", err)
	}
	if got != (remoteRelease{FullBuild: remoteFullBuild, BuildConfig: remoteBuildKey, CDNConfig: remoteCDNKey}) {
		t.Fatalf("parseReleaseCatalog() = %#v", got)
	}
}

func TestParseReleaseCatalogSelectionErrors(t *testing.T) {
	base := "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\n"
	tests := []struct {
		name  string
		body  string
		build string
		want  error
	}{
		{"duplicate selected region", "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\nus|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\n", "", ErrBuildAmbiguous},
		{"unknown build with duplicate region", "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\nus|" + remoteBuildKey + "|" + remoteCDNKey + "|69876|12.1.0.69876\n", "12.1.0.11111", ErrBuildUnavailable},
		{"wrong build does not use another row", "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\neu|" + remoteBuildKey + "|" + remoteCDNKey + "|69876|12.1.0.69876\n", "12.1.0.69876", ErrBuildUnavailable},
		{"no fallback latest", "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\n", "12.1.0.69874", ErrBuildUnavailable},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, err := parseReleaseCatalog([]byte(base+test.body), remoteRegion, test.build)
			if !errors.Is(err, test.want) {
				t.Fatalf("error = %v, want %v", err, test.want)
			}
		})
	}
}

const remoteReleaseHeader = "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\n"

// releaseRow builds one versions-manifest row for the fixture region.
func releaseRow(buildConfig, cdnConfig, buildID, versionsName string) string {
	return remoteRegion + "|" + buildConfig + "|" + cdnConfig + "|" + buildID + "|" + versionsName + "\n"
}

func checkReleaseSelection(t *testing.T, raw, build string, want remoteRelease, wantErr error) {
	t.Helper()
	got, err := parseReleaseCatalog([]byte(raw), remoteRegion, build)
	if wantErr != nil {
		if !errors.Is(err, wantErr) || got != (remoteRelease{}) {
			t.Fatalf("parseReleaseCatalog() = %#v, %v, want error %v", got, err, wantErr)
		}
		return
	}
	if err != nil || got != want {
		t.Fatalf("parseReleaseCatalog() = %#v, %v, want %#v", got, err, want)
	}
}

// The historical remote build rule (decision 13): an exact full --build names
// one historical manifest row and constrains the region's rows before any
// other selection, so historical builds resolve while unknown builds stay
// unavailable.
func TestParseRemoteReleaseCatalogHistoricalBuildRule(t *testing.T) {
	const (
		keyA = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
		keyB = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		keyC = "cccccccccccccccccccccccccccccccc"
		keyD = "dddddddddddddddddddddddddddddddd"
		keyE = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
		keyF = "ffffffffffffffffffffffffffffffff"
	)
	current := releaseRow(keyA, keyB, "69876", "12.1.0.69876")
	middle := releaseRow(keyC, keyD, "69875", "12.1.0.69875")
	historical := releaseRow(keyE, keyF, "69874", "12.1.0.69874")
	// The region's manifest lists historical rows; listed order decides nothing.
	multi := remoteReleaseHeader + historical + current + middle

	t.Run("historical build resolves to its exact row", func(t *testing.T) {
		checkReleaseSelection(t, multi, "12.1.0.69874", remoteRelease{FullBuild: "12.1.0.69874", BuildConfig: keyE, CDNConfig: keyF}, nil)
	})
	t.Run("every listed build resolves to its exact row", func(t *testing.T) {
		checkReleaseSelection(t, multi, "12.1.0.69875", remoteRelease{FullBuild: "12.1.0.69875", BuildConfig: keyC, CDNConfig: keyD}, nil)
		checkReleaseSelection(t, multi, "12.1.0.69876", remoteRelease{FullBuild: "12.1.0.69876", BuildConfig: keyA, CDNConfig: keyB}, nil)
	})
	t.Run("unknown build is unavailable not ambiguous", func(t *testing.T) {
		checkReleaseSelection(t, multi, "12.1.0.11111", remoteRelease{}, ErrBuildUnavailable)
	})
	t.Run("duplicate rows for one exact build are ambiguous", func(t *testing.T) {
		duplicate := remoteReleaseHeader + middle + current + releaseRow(keyA, keyB, "69875", "12.1.0.69875")
		checkReleaseSelection(t, duplicate, "12.1.0.69875", remoteRelease{}, ErrBuildAmbiguous)
	})
	t.Run("single row region unchanged", func(t *testing.T) {
		checkReleaseSelection(t, remoteReleaseHeader+historical, "12.1.0.69874", remoteRelease{FullBuild: "12.1.0.69874", BuildConfig: keyE, CDNConfig: keyF}, nil)
		checkReleaseSelection(t, remoteReleaseHeader+historical, "12.1.0.69875", remoteRelease{}, ErrBuildUnavailable)
	})
}

// Without --build the region's current manifest row is the release: the unique
// newest VersionsName. Several historical rows are fine; only a current row
// that cannot be identified uniquely is ambiguous.
func TestParseRemoteReleaseCatalogCurrentRowRule(t *testing.T) {
	const (
		keyA = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
		keyB = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		keyC = "cccccccccccccccccccccccccccccccc"
		keyD = "dddddddddddddddddddddddddddddddd"
		keyE = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
		keyF = "ffffffffffffffffffffffffffffffff"
	)
	current := releaseRow(keyA, keyB, "69876", "12.1.0.69876")
	middle := releaseRow(keyC, keyD, "69875", "12.1.0.69875")
	historical := releaseRow(keyE, keyF, "69874", "12.1.0.69874")

	t.Run("multi row region picks the current row", func(t *testing.T) {
		checkReleaseSelection(t, remoteReleaseHeader+historical+current+middle, "", remoteRelease{FullBuild: "12.1.0.69876", BuildConfig: keyA, CDNConfig: keyB}, nil)
	})
	t.Run("unidentifiable current row is ambiguous", func(t *testing.T) {
		tied := remoteReleaseHeader + current + middle + releaseRow(keyC, keyD, "69876", "12.1.0.69876")
		checkReleaseSelection(t, tied, "", remoteRelease{}, ErrBuildAmbiguous)
	})
	t.Run("rows without a well-formed build claim no currency", func(t *testing.T) {
		none := remoteReleaseHeader + releaseRow(keyA, keyB, "x", "not-a-build") + releaseRow(keyC, keyD, "x", "also-not-a-build")
		checkReleaseSelection(t, none, "", remoteRelease{}, ErrBuildAmbiguous)
		mixed := remoteReleaseHeader + releaseRow(keyA, keyB, "x", "12.1.x.69890") + historical
		checkReleaseSelection(t, mixed, "", remoteRelease{FullBuild: "12.1.0.69874", BuildConfig: keyE, CDNConfig: keyF}, nil)
	})
	t.Run("single row region unchanged", func(t *testing.T) {
		checkReleaseSelection(t, remoteReleaseHeader+historical, "", remoteRelease{FullBuild: "12.1.0.69874", BuildConfig: keyE, CDNConfig: keyF}, nil)
	})
}

func TestParseReleaseCatalogValidatesOnlySelectedRow(t *testing.T) {
	raw := "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\n" +
		"eu|not-md5|not-md5|wrong|not-a-build\n" +
		"us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\n"
	got, err := parseReleaseCatalog([]byte(raw), remoteRegion, "")
	if err != nil || got.FullBuild != remoteFullBuild {
		t.Fatalf("valid selected row rejected: %#v, %v", got, err)
	}

	for name, body := range map[string]string{
		"bad key":   "us|not-md5|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\n",
		"bad build": "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|12.1.x.69875\n",
		"bad id":    "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69874|" + remoteFullBuild + "\n",
	} {
		t.Run(name, func(t *testing.T) {
			_, err := parseReleaseCatalog([]byte("Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\n"+body), remoteRegion, "")
			if !errors.Is(err, ErrMetadataFormat) {
				t.Fatalf("error = %v, want ErrMetadataFormat", err)
			}
		})
	}
}

func TestParseDistributionCatalog(t *testing.T) {
	raw := "\ufeff# CDN catalog\r\nName!STRING:0|Path!STRING:0|Hosts!STRING:0\r\nus|tpr/wow/12_1|cdn01.example.com cdn02.example.com\r\neu|ignored/path|bad host\r\n"
	got, err := parseDistributionCatalog([]byte(raw), remoteRegion)
	if err != nil {
		t.Fatalf("parseDistributionCatalog() error = %v", err)
	}
	want := distributionRoute{Path: "tpr/wow/12_1", Hosts: []string{"cdn01.example.com", "cdn02.example.com"}}
	if fmt.Sprint(got) != fmt.Sprint(want) {
		t.Fatalf("parseDistributionCatalog() = %#v, want %#v", got, want)
	}
}

func TestParseDistributionCatalogRejectsUnsafeValues(t *testing.T) {
	tests := map[string]string{
		"empty hosts":     "tpr/wow|\n",
		"duplicate hosts": "tpr/wow|cdn.example.com CDN.EXAMPLE.COM\n",
		"port":            "tpr/wow|cdn.example.com:443\n",
		"url":             "tpr/wow|https://cdn.example.com\n",
		"ip literal":      "tpr/wow|127.0.0.1\n",
		"localhost":       "tpr/wow|localhost\n",
		"single numeric":  "tpr/wow|123\n",
		"userinfo":        "tpr/wow|user@cdn.example.com\n",
		"query":           "tpr/wow|cdn.example.com?x=1\n",
		"host slash":      "tpr/wow|cdn.example.com/path\n",
		"host fragment":   "tpr/wow|cdn.example.com#fragment\n",
		"bad path":        "tpr/../wow|cdn.example.com\n",
		"leading slash":   "/tpr/wow|cdn.example.com\n",
		"trailing slash":  "tpr/wow/|cdn.example.com\n",
	}
	for name, row := range tests {
		t.Run(name, func(t *testing.T) {
			raw := "Name!STRING:0|Path!STRING:0|Hosts!STRING:0\n" + "us|" + row
			_, err := parseDistributionCatalog([]byte(raw), remoteRegion)
			if !errors.Is(err, ErrMetadataFormat) {
				t.Fatalf("error = %v, want ErrMetadataFormat", err)
			}
		})
	}
}

func TestParseRemoteBPSVRejectsMalformedHeadersAndRows(t *testing.T) {
	validHeader := "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\n"
	validRow := "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\n"
	tests := map[string][]byte{
		"duplicate header": []byte("Region!STRING:0|Region!STRING:0\n"),
		"empty header":     []byte("Region!STRING:0||BuildConfig!HEX:16\n"),
		"missing type":     []byte("Region|BuildConfig!HEX:16\n"),
		"missing required": []byte("Region!STRING:0|BuildConfig!HEX:16\n"),
		"short row":        []byte(validHeader + "us|" + remoteBuildKey + "\n"),
		"long row":         []byte(validHeader + validRow[:len(validRow)-1] + "|tail\n"),
		"bad tail":         []byte(validHeader + validRow + "other|row\n"),
		"invalid utf8":     append([]byte(validHeader), 0xff),
		"nul":              append([]byte(validHeader), 0),
	}
	for name, raw := range tests {
		t.Run(name, func(t *testing.T) {
			_, err := parseReleaseCatalog(raw, remoteRegion, "")
			if !errors.Is(err, ErrMetadataFormat) {
				t.Fatalf("error = %v, want ErrMetadataFormat", err)
			}
		})
	}
}

func TestParseRemoteBPSVLimits(t *testing.T) {
	if _, err := parseReleaseCatalog(bytes.Repeat([]byte("x"), remoteCatalogMaxBytes+1), remoteRegion, ""); !errors.Is(err, ErrMetadataLimit) {
		t.Fatalf("oversized input error = %v, want ErrMetadataLimit", err)
	}

	tooManyColumns := make([]string, remoteCatalogMaxColumns+1)
	for i := range tooManyColumns {
		tooManyColumns[i] = fmt.Sprintf("C%d!STRING:0", i)
	}
	if _, err := parseReleaseCatalog([]byte(strings.Join(tooManyColumns, "|")), remoteRegion, ""); !errors.Is(err, ErrMetadataLimit) {
		t.Fatalf("too many columns error = %v, want ErrMetadataLimit", err)
	}

	header := "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\n"
	row := "eu|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\n"
	withinLimit := header + strings.Repeat(row, remoteCatalogMaxRows-1) + "us|" + remoteBuildKey + "|" + remoteCDNKey + "|69875|" + remoteFullBuild + "\n"
	if _, err := parseReleaseCatalog([]byte(withinLimit), remoteRegion, ""); err != nil {
		t.Fatalf("4096 rows rejected: %v", err)
	}
	tooManyRows := withinLimit + row
	if _, err := parseReleaseCatalog([]byte(tooManyRows), remoteRegion, ""); !errors.Is(err, ErrMetadataLimit) {
		t.Fatalf("too many rows error = %v, want ErrMetadataLimit", err)
	}
}

func TestDistributionBoundaries(t *testing.T) {
	maxHost := strings.Repeat("a", 63) + "." + strings.Repeat("b", 63) + "." + strings.Repeat("c", 63) + "." + strings.Repeat("d", 61)
	maxPath := strings.Repeat("a", 256)
	raw := "Name!STRING:0|Path!STRING:0|Hosts!STRING:0\nus|" + maxPath + "|" + maxHost + "\n"
	if _, err := parseDistributionCatalog([]byte(raw), remoteRegion); err != nil {
		t.Fatalf("maximum valid path/host rejected: %v", err)
	}

	tooManyHosts := make([]string, 17)
	for i := range tooManyHosts {
		tooManyHosts[i] = fmt.Sprintf("cdn%d.example.com", i)
	}
	raw = "Name!STRING:0|Path!STRING:0|Hosts!STRING:0\nus|tpr/wow|" + strings.Join(tooManyHosts, " ") + "\n"
	if _, err := parseDistributionCatalog([]byte(raw), remoteRegion); !errors.Is(err, ErrMetadataFormat) {
		t.Fatalf("17 hosts error = %v, want ErrMetadataFormat", err)
	}
}
