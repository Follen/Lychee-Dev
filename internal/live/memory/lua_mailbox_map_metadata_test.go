package memory

import (
	"context"
	"encoding/binary"
	"testing"
)

// Match the producer's fresh schema-only map with one allocated hash bucket.
// All fixture allocations are private; the reader needs no static-node access.
func fixtureSchemaOnlyMap(f *mailboxFixture) uint64 {
	table, h := f.allocate(72)
	h[16], h[19] = 5, 0
	nodes, node := f.allocate(56)
	binary.LittleEndian.PutUint64(h[40:48], nodes)
	binary.LittleEndian.PutUint64(h[48:56], nodes+56)
	binary.LittleEndian.PutUint64(node[:8], f.text([]byte(MailboxSchema)))
	node[8] = 4
	binary.LittleEndian.PutUint64(node[24:32], f.text([]byte("schema")))
	node[32] = 4
	return table
}

func TestMailboxSchemaOnlyMapsKeepMissingRecordsComplete(t *testing.T) {
	for _, field := range []string{"receipts", "bodies"} {
		t.Run(field, func(t *testing.T) {
			f := newMailboxFixture(t, 0x140000000, 0x200000000)
			binary.LittleEndian.PutUint64(f.bytes(f.nodes["box."+field], 8), fixtureSchemaOnlyMap(f))
			selector := f.selectors["receipt"]
			if field == "bodies" {
				selector = f.selectors["body"]
			} // Existing verified HEAD qualification.
			found, err := f.reader().Lookup(context.Background(), selector)
			if err != nil || len(found.Records) != 0 || !found.Coverage.Complete || found.FirstReadFailure != nil || f.regions != 0 {
				t.Fatalf("schema-only %s map turned missing token into a record/read failure: %+v %v", field, found, err)
			}
			if found.Coverage.Workers[0].ReadCalls != uint64(f.rootReads+f.privateReads) {
				t.Fatal("missing-record lookup bypassed the existing accounting")
			}
		})
	}
}
