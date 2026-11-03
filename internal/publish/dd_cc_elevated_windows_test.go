//go:build windows && amd64

package publish

import (
	"context"
	"encoding/json"
	"os"
	"testing"
)

func TestDDChannelHelperOnlyRecognizesFixedPrivateArguments(t *testing.T) {
	if handled, _ := ddHandleChannelHelper(context.Background(), []string{"describe"}); handled {
		t.Fatal("intercepted public CLI")
	}
	if handled, exit := ddHandleChannelHelper(context.Background(), []string{ddChannelFlag}); !handled || exit != 2 {
		t.Fatal("private argument admission")
	}
	nonce := ddRandom()
	args := []string{ddChannelFlag, `\\.\pipe\unrelated`, nonce, "1", "1", "1"}
	if ddRunChannelHelper(context.Background(), args) == nil {
		t.Fatal("accepted unrelated pipe")
	}
	args[1] = `\\.\pipe\lycheedev-dd-channels-` + nonce
	args[5] = "9999999999999"
	if ddRunChannelHelper(context.Background(), args) == nil {
		t.Fatal("accepted extended deadline")
	}
	args[5] = "1"
	if ddRunChannelHelper(context.Background(), args) == nil {
		t.Fatal("accepted expired deadline")
	}
	if err := ddSameUser(uint32(os.Getpid())); err != nil {
		t.Fatal(err)
	}
}
func TestDDChannelHelperRejectsExtraPrivateFieldsAndChangedDependencies(t *testing.T) {
	raw := `{"total":1,"items":[{"room_id":"1","room_name":"房间","channel_id":"2","channel_name":"频道","channel_type":"1"}],"dependencies":[{"parent":"room_id","children":["channel_id","channel_type"]}]}`
	var value any
	ddJSON([]byte(raw), &value)
	if err := ddValidateChannelChoices(value); err != nil {
		t.Fatal(err)
	}
	m := value.(map[string]any)
	m["private_token"] = "synthetic"
	if ddValidateChannelChoices(value) == nil {
		t.Fatal("allowed private helper field")
	}
	delete(m, "private_token")
	m["total"] = json.Number("2")
	if ddValidateChannelChoices(value) == nil {
		t.Fatal("wrong item count")
	}
	m["total"] = json.Number("1")
	m["dependencies"] = []any{}
	if ddValidateChannelChoices(value) == nil {
		t.Fatal("lost parent identity")
	}
}
