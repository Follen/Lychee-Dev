package records

import "testing"

func TestUnavailableIdentityCoverage(t *testing.T) {
	for _, tc := range []struct {
		name    string
		parts   []UnavailablePartition
		id      uint32
		key     string
		unknown bool
	}{
		{"known", []UnavailablePartition{{KeyID: "key", RecordIDs: []uint32{2, 3}, IDsComplete: true}}, 2, "key", false},
		{"absent", []UnavailablePartition{{KeyID: "key", RecordIDs: []uint32{2, 3}, IDsComplete: true}}, 4, "", false},
		{"legacy", []UnavailablePartition{{KeyID: "key"}}, 4, "", true},
		{"partial-list", []UnavailablePartition{{KeyID: "key", RecordIDs: []uint32{2}}}, 4, "", true},
		{"known-in-partial-list", []UnavailablePartition{{KeyID: "key", RecordIDs: []uint32{2}}}, 2, "key", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			key, unknown := unavailableRecord(tc.parts, tc.id)
			if key != tc.key || unknown != tc.unknown {
				t.Fatalf("%q %v", key, unknown)
			}
		})
	}
}
