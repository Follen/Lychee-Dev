package records

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"io"
	"sort"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

type rootGroup struct {
	ids                              []uint32
	keysStart, namesStart, keyStride int64
	hasName                          bool
	content, locale                  uint32
	group                            int
}

type rootIndex struct {
	source io.ReaderAt
	size   int64
	limits RootLimits
	groups []rootGroup
}

// Root framing and every delta are validated once by the shared parser. Compact
// FDID arrays support binary lookup without one large Go map per game file.
func newRootIndex(ctx context.Context, source io.ReaderAt, size int64, limits RootLimits) (*rootIndex, error) {
	index := &rootIndex{source: source, size: size, limits: limits}
	if _, err := lookupRoot(ctx, source, size, nil, limits, index); err != nil {
		return nil, err
	}
	return index, nil
}

func (r *rootIndex) lookup(ctx context.Context, id uint32) ([]RootRecord, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	release, err := r.limits.Query.ReserveScratch(int64(r.limits.Matches) * 256)
	if err != nil {
		return nil, err
	}
	defer release()
	input := rootInput{ctx: ctx, source: r.source, size: r.size}
	var result []RootRecord
	for _, group := range r.groups {
		if err := r.limits.Query.Charge(resource.Cost{DecodeWork: 1}); err != nil {
			return nil, err
		}
		at := sort.Search(len(group.ids), func(i int) bool { return group.ids[i] >= id })
		if at == len(group.ids) || group.ids[at] != id {
			continue
		}
		if len(result) >= r.limits.Matches {
			return nil, ErrMetadataLimit
		}
		var key [24]byte
		if err := input.read(key[:group.keyStride], group.keysStart+int64(at)*group.keyStride); err != nil {
			return nil, err
		}
		record := RootRecord{FileDataID: id, ContentKey: hex.EncodeToString(key[:16]), LocaleMask: group.locale, ContentFlags: group.content, HasNameHash: group.hasName, Group: group.group}
		if group.hasName {
			if group.keyStride != 24 {
				if err := input.read(key[16:24], group.namesStart+int64(at)*8); err != nil {
					return nil, err
				}
			}
			record.NameHash = binary.LittleEndian.Uint64(key[16:24])
		}
		result = append(result, record)
	}
	return result, ctx.Err()
}
