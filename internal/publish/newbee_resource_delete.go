package publish

import (
	"context"
	"encoding/json"
	"fmt"
)

func (c *newBeeCreator) verifyDeletedResource(ctx context.Context, resource string, id int64) (any, error) {
	offset, page := int64(0), int64(1)
	seen := map[int64]bool{}
	total := int64(-1)
	for {
		input := map[string]any{"page_size": json.Number("100")}
		if resource == "plugin" {
			input["page"] = json.Number(fmt.Sprint(page))
		} else {
			input["offset"] = json.Number(fmt.Sprint(offset))
		}
		value, err := c.read(ctx, resource+" list", input)
		if err != nil {
			return nil, err
		}
		object := newBeeObject(value)
		rows := newBeeRows(value)
		countValue := newBeePick(object, "total", "count")
		count := newBeeID(countValue)
		if rows == nil || countValue == nil || count < 0 || count > 100000 || total >= 0 && total != count {
			return nil, &Error{Code: "publish.newbee_verification", Message: "author list pagination is incomplete", Exit: 6}
		}
		total = count
		for _, row := range rows {
			record := newBeeObject(row)
			current := newBeeID(newBeePick(record, "id", "t_id", "mod_id", "wa_id"))
			if current == id {
				return nil, &Error{Code: "publish.newbee_verification", Message: "deleted resource remains in author list", Exit: 6}
			}
			if current <= 0 || seen[current] {
				return nil, &Error{Code: "publish.newbee_verification", Message: "author list repeats or omits resource IDs", Exit: 6}
			}
			seen[current] = true
		}
		if int64(len(seen)) == total {
			return map[string]any{"id": id, "present": false, "coverage_complete": true}, nil
		}
		if len(rows) == 0 || int64(len(seen)) > total {
			return nil, &Error{Code: "publish.newbee_verification", Message: "author list ended before declared total", Exit: 6}
		}
		next := offset + int64(len(rows))
		if raw, ok := object["next_offset"]; ok {
			next = newBeeID(raw)
		}
		if next <= offset {
			return nil, &Error{Code: "publish.newbee_verification", Message: "author list cursor did not advance", Exit: 6}
		}
		offset = next
		page++
	}
}
