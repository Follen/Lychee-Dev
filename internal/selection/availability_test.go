package selection

import (
	"context"
	"errors"
	"strings"
	"testing"
)

func manifestFixture() []byte {
	header := "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:4|VersionsName!STRING:0\n"
	rows := strings.Join([]string{
		"## seqn = 1",
		"cn|" + strings.Repeat("1", 32) + "|" + strings.Repeat("2", 32) + "|69875|12.1.0.69875",
		"cn|" + strings.Repeat("3", 32) + "|" + strings.Repeat("4", 32) + "|69874|12.1.0.69874",
		"us,eu|" + strings.Repeat("5", 32) + "|" + strings.Repeat("6", 32) + "|69873|12.1.0.69873",
		"cn|broken|broken|x|not-a-build",
		"cn|" + strings.Repeat("7", 32) + "|" + strings.Repeat("8", 32) + "|99999|12.1.0.69872",
	}, "\n")
	return []byte(header + rows + "\n")
}

func TestListRemoteAvailabilityParsesAndBounds(t *testing.T) {
	calls := make([]string, 0)
	fetch := func(ctx context.Context, region, productCode string) ([]byte, error) {
		calls = append(calls, region+"/"+productCode)
		if productCode == "wow_classic" {
			return nil, errors.New("offline fixture failure")
		}
		return manifestFixture(), nil
	}
	result, err := ListRemoteAvailability(context.Background(), AvailabilityOptions{Region: "cn", Fetch: fetch})
	if err != nil {
		t.Fatalf("ListRemoteAvailability() error = %v", err)
	}
	if len(calls) != 4 {
		t.Fatalf("bounded listing made %d fetches: %v", len(calls), calls)
	}
	wow := result.Products[0]
	if wow.Product != "retail" || wow.ProductCode != "wow" {
		t.Fatalf("product order wrong: %v", wow)
	}
	// cn rows only: newest first, malformed and mismatched rows rejected.
	if len(wow.Rows) != 2 || wow.Rows[0].FullBuild != "12.1.0.69875" || !wow.Rows[0].Current || wow.Rows[1].Current {
		t.Fatalf("rows = %+v", wow.Rows)
	}
	if wow.Rejected != 2 {
		t.Fatalf("rejected = %d, want 2", wow.Rejected)
	}
	if result.Products[1].Error == "" {
		t.Fatalf("failing product not reported: %+v", result.Products[1])
	}
}

func TestListRemoteAvailabilityRefusesOffline(t *testing.T) {
	called := false
	_, err := ListRemoteAvailability(context.Background(), AvailabilityOptions{
		Region: "us", Offline: true,
		Fetch: func(context.Context, string, string) ([]byte, error) {
			called = true
			return nil, nil
		},
	})
	if !errors.Is(err, ErrListingOffline) {
		t.Fatalf("offline listing error = %v, want %v", err, ErrListingOffline)
	}
	if called {
		t.Fatal("offline listing touched the network")
	}
}

func TestSelectReleaseHistoricalBuildRule(t *testing.T) {
	rows, rejected, err := ParseVersionsManifest(manifestFixture(), "cn")
	if err != nil || rejected != 2 {
		t.Fatalf("ParseVersionsManifest() = %v, %d, %v", rows, rejected, err)
	}
	current, err := SelectRelease(rows, "")
	if err != nil || current.FullBuild != "12.1.0.69875" {
		t.Fatalf("SelectRelease() = %v, %v", current, err)
	}
	historical, err := SelectRelease(rows, "12.1.0.69874")
	if err != nil || historical.FullBuild != "12.1.0.69874" {
		t.Fatalf("SelectRelease(historical) = %v, %v", historical, err)
	}
	if _, err := SelectRelease(rows, "12.1.0.60000"); !errors.Is(err, ErrTargetBuildUnavailable) {
		t.Fatalf("missing build error = %v, want %v", err, ErrTargetBuildUnavailable)
	}
	if _, err := SelectRelease(nil, ""); !errors.Is(err, ErrTargetBuildUnavailable) {
		t.Fatalf("empty catalog error = %v, want %v", err, ErrTargetBuildUnavailable)
	}
}

func TestParseVersionsManifestRejectsMalformed(t *testing.T) {
	if _, _, err := ParseVersionsManifest([]byte("nonsense"), "cn"); !errors.Is(err, ErrManifestFormat) {
		t.Fatalf("error = %v, want %v", err, ErrManifestFormat)
	}
	if _, _, err := ParseVersionsManifest(make([]byte, manifestMaxBytes+1), "cn"); !errors.Is(err, ErrManifestLimit) {
		t.Fatalf("error = %v, want %v", err, ErrManifestLimit)
	}
}
