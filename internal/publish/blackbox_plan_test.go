package publish

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

func TestBlackboxPluginEditPlanAdmitsOnlyOfficialTypes(t *testing.T) {
	for value := 1; value <= 9; value++ {
		t.Run(strconv.Itoa(value), func(t *testing.T) {
			spec, _ := BlackboxInput("plugin edit")
			in := spec.Example
			in["type"] = value
			raw, err := json.Marshal(in)
			if err != nil {
				t.Fatal(err)
			}
			home := filepath.Join(t.TempDir(), "absent-home")
			result, err := RunBlackbox(context.Background(), "plugin edit", Request{Input: raw, Home: home, Plan: true})
			if value == 1 || value == 9 {
				if err != nil || !result.Complete {
					t.Fatalf("official type rejected: %+v %v", result, err)
				}
			} else {
				var pe *Error
				if !errors.As(err, &pe) || pe.Exit != 2 || result.Complete {
					t.Errorf("unofficial type accepted: %+v %v", result, err)
				}
			}
			if _, err := os.Stat(home); !os.IsNotExist(err) {
				t.Fatal("plan initialized home")
			}
		})
	}
}

func TestBlackboxConfigCreatePlanChecksLocalContent(t *testing.T) {
	for _, tc := range []struct {
		name   string
		change func(map[string]any)
	}{
		{"public-without-preview", func(c map[string]any) { c["Open"] = true }},
		{"empty-selection", func(c map[string]any) { c["KnownAddons"] = []any{} }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			spec, _ := BlackboxInput("config create")
			in := spec.Example
			tc.change(in["configuration"].(map[string]any))
			home := filepath.Join(t.TempDir(), "absent-home")
			raw, _ := json.Marshal(in)
			result, err := RunBlackbox(context.Background(), "config create", Request{Home: home, Input: raw, Plan: true})
			var pe *Error
			if !errors.As(err, &pe) || pe.Exit != 2 || result.Complete {
				t.Errorf("invalid configuration plan accepted: %+v, %v", result, err)
			}
			in["configuration"].(map[string]any)["ThumbList"] = []any{"https://example.com/preview.png"}
			in["configuration"].(map[string]any)["KnownAddons"] = []any{map[string]any{"ID": "1"}}
			raw, _ = json.Marshal(in)
			result, err = RunBlackbox(context.Background(), "config create", Request{Home: home, Input: raw, Plan: true})
			if err != nil || !result.Complete {
				t.Fatalf("valid configuration plan: %+v, %v", result, err)
			}
			if _, err := os.Stat(home); !os.IsNotExist(err) {
				t.Fatal("plan initialized home")
			}
		})
	}
}

func TestBlackboxConfigTitleWeightedLimit(t *testing.T) {
	for _, tc := range []struct {
		name, title string
		valid       bool
	}{
		{"ascii-over", strings.Repeat("a", 71), false},
		{"bmp-over", strings.Repeat("中", 36), false},
		{"astral-over", strings.Repeat("😀", 18), false},
		{"ascii-boundary", strings.Repeat("a", 70), true},
		{"bmp-boundary", strings.Repeat("中", 35), true},
		{"astral-boundary", strings.Repeat("😀", 17) + "aa", true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			spec, _ := BlackboxInput("config create")
			in := spec.Example
			in["configuration"].(map[string]any)["Title"] = tc.title
			raw, _ := json.Marshal(in)
			result, err := RunBlackbox(context.Background(), "config create", Request{Input: raw, Plan: true})
			if (err == nil && result.Complete) != tc.valid {
				t.Errorf("plan title admission: complete=%t err=%v", result.Complete, err)
			}
			if got := bbValidateConfigInput(in["configuration"].(map[string]any)); (got == nil) != tc.valid {
				t.Errorf("effective configuration admission: %v", got)
			}
		})
	}
}

func TestBlackboxAuthAndPreviewPlanUsesLocalAdmission(t *testing.T) {
	for _, tc := range []struct {
		name, action string
		bad, good    map[string]any
	}{
		{"forget", "session forget", map[string]any{"confirm_forget": false}, map[string]any{"confirm_forget": true}},
		{"missing-phone", "session login-start", map[string]any{"method": "sms"}, map[string]any{"method": "sms", "phone": "13800000000"}},
		{"invalid-phone", "session login-start", map[string]any{"method": "sms", "phone": "00000000000"}, map[string]any{"method": "sms", "phone": "13800000000"}},
		{"wechat-phone", "session login-start", map[string]any{"method": "wechat", "phone": "13800000000"}, map[string]any{"method": "wechat"}},
		{"otp", "session login-submit", map[string]any{"login_id": strings.Repeat("a", 32), "code": "abcd"}, map[string]any{"login_id": strings.Repeat("a", 32), "code": "123456"}},
		{"login-id", "session login-status", map[string]any{"login_id": "other"}, map[string]any{"login_id": strings.Repeat("a", 32)}},
		{"missing-selection", "backup delete-preview", map[string]any{}, map[string]any{"name_contains": "example"}},
		{"mixed-selection", "backup delete-preview", map[string]any{"backup_ids": []any{"1"}, "name_contains": "example"}, map[string]any{"backup_ids": []any{"1"}}},
		{"duplicate-selection", "backup delete-preview", map[string]any{"backup_ids": []any{"1", "1"}}, map[string]any{"backup_ids": []any{"1", "2"}}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			home := filepath.Join(t.TempDir(), "absent-home")
			for i, in := range []map[string]any{tc.bad, tc.good} {
				raw, _ := json.Marshal(in)
				result, err := RunBlackbox(context.Background(), tc.action, Request{Home: home, Input: raw, Plan: true})
				if i == 0 {
					var pe *Error
					if !errors.As(err, &pe) || pe.Exit != 2 || result.Complete {
						t.Errorf("invalid plan accepted: %+v %v", result, err)
					}
				} else if err != nil || !result.Complete {
					t.Errorf("valid plan rejected: %+v %v", result, err)
				}
				if _, err := os.Stat(home); !os.IsNotExist(err) {
					t.Fatal("plan initialized home")
				}
			}
		})
	}
}

func TestBlackboxPlanRejectsLocalDeleteAndWACreationErrors(t *testing.T) {
	cases := []struct {
		name, action string
		mutate       func(map[string]any)
	}{
		{"config-confirmation", "config delete", func(in map[string]any) { in["confirm_delete"] = false }},
		{"wa-confirmation", "wa delete", func(in map[string]any) { in["confirm_delete"] = false }},
		{"backup-confirmation", "backup delete", func(in map[string]any) { in["confirm_delete"] = false }},
		{"weighted-name", "wa create", func(in map[string]any) { in["configuration"].(map[string]any)["name"] = strings.Repeat("中", 36) }},
		{"duplicate-category", "wa create", func(in map[string]any) {
			in["configuration"].(map[string]any)["categories"] = []any{"example", "example"}
		}},
		{"invalid-preview", "wa create", func(in map[string]any) {
			in["configuration"].(map[string]any)["screens"] = []any{map[string]any{"src": "http://example.com/image.png", "thumb": "https://example.com/image.png"}}
		}},
		{"invalid-branch", "wa create", func(in map[string]any) { in["configuration"].(map[string]any)["strappid"] = "other" }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			home := filepath.Join(t.TempDir(), "absent-home")
			for _, valid := range []bool{false, true} {
				spec, _ := BlackboxInput(tc.action)
				in := spec.Example
				if !valid {
					tc.mutate(in)
				}
				raw, err := json.Marshal(in)
				if err != nil {
					t.Fatal(err)
				}
				result, err := RunBlackbox(context.Background(), tc.action, Request{Input: raw, Home: home, Plan: true})
				if valid {
					if err != nil || !result.Complete {
						t.Fatalf("valid plan: %+v, %v", result, err)
					}
				} else {
					var pe *Error
					if !errors.As(err, &pe) || pe.Exit != 2 || result.Complete {
						t.Errorf("invalid local plan accepted: %+v, %v", result, err)
					}
				}
				if _, err := os.Stat(home); !os.IsNotExist(err) {
					t.Fatal("plan initialized home")
				}
			}
		})
	}
}

func TestBlackboxPlanReusesLocalAdmissionAcrossActions(t *testing.T) {
	for _, action := range []string{"backup delete", "config edit", "wa edit", "plugin update", "version edit", "backup scan", "backup generate", "backup create", "backup upload"} {
		t.Run(action, func(t *testing.T) {
			spec, _ := BlackboxInput(action)
			in := spec.Example
			home := filepath.Join(t.TempDir(), "absent-home")
			switch action {
			case "backup delete":
				in["backup_ids"] = []any{"same", "same"}
			case "config edit":
				in["configuration"].(map[string]any)["Title"] = strings.Repeat("a", 71)
			case "wa edit":
				in["configuration"].(map[string]any)["categories"] = []any{"same", "same"}
			case "plugin update", "version edit", "backup upload":
				in["file"] = filepath.Join(t.TempDir(), "missing.zip")
			case "backup scan":
				in["game_dir"] = filepath.Join(t.TempDir(), "missing-game")
			case "backup generate", "backup create":
				in, _ = bbBackupFixture(t)
				in["output_dir"] = filepath.Join(in["game_dir"].(string), "backup")
			}
			raw, _ := json.Marshal(in)
			result, err := RunBlackbox(context.Background(), action, Request{Home: home, Input: raw, Plan: true})
			if err == nil || result.Complete {
				t.Errorf("invalid local plan accepted: %+v %v", result, err)
			}
			if _, err := os.Stat(home); !os.IsNotExist(err) {
				t.Fatal("plan initialized home")
			}
		})
	}
}

func TestBlackboxLocalPlansLeaveFilesAndHomeUntouched(t *testing.T) {
	in, _ := bbBackupFixture(t)
	home := filepath.Join(t.TempDir(), "absent-home")
	output := String(in, "output_dir")
	run := func(action string, input map[string]any) {
		t.Helper()
		raw, _ := json.Marshal(input)
		result, err := RunBlackbox(context.Background(), action, Request{Home: home, Input: raw, Plan: true})
		if err != nil || !result.Complete {
			t.Fatalf("%s: %+v %v", action, result, err)
		}
		if _, err := os.Stat(home); !os.IsNotExist(err) {
			t.Fatal("plan created home or artifact snapshot")
		}
	}
	run("backup scan", map[string]any{"game_dir": in["game_dir"], "strappid": in["strappid"]})
	run("backup generate", in)
	run("backup create", in)
	if _, err := os.Stat(output); !os.IsNotExist(err) {
		t.Fatal("plan created backup output")
	}
	generated, err := bbLocalBackup(context.Background(), "backup generate", in)
	if err != nil {
		t.Fatal(err)
	}
	packageFile := String(generated.(map[string]any), "file")
	before, err := os.ReadFile(packageFile)
	if err != nil {
		t.Fatal(err)
	}
	for _, action := range []string{"backup upload", "backup create"} {
		run(action, map[string]any{"file": packageFile})
	}
	after, _ := os.ReadFile(packageFile)
	if string(before) != string(after) {
		t.Fatal("plan changed package manifest")
	}
	for _, action := range []string{"plugin update", "version edit"} {
		spec, _ := BlackboxInput(action)
		input := spec.Example
		input["file"] = filepath.Join(output, "unknown.zip")
		run(action, input)
	}
	for _, action := range []string{"config edit", "wa edit", "backup delete"} {
		spec, _ := BlackboxInput(action)
		run(action, spec.Example)
	}
}

func TestBlackboxPartialEditPlanValidatesOnlySuppliedFields(t *testing.T) {
	for _, tc := range []struct {
		action, key string
		value       any
	}{
		{"config edit", "Title", strings.Repeat("中", 36)},
		{"config edit", "ThumbList", []any{}},
		{"wa edit", "name", strings.Repeat("中", 36)},
		{"wa edit", "categories", []any{"same", "same"}},
		{"wa edit", "strappid", "invalid"},
		{"wa edit", "screens", []any{map[string]any{"src": "http://example.com/a.png", "thumb": "https://example.com/a.png"}}},
	} {
		t.Run(tc.action+"-"+tc.key, func(t *testing.T) {
			spec, _ := BlackboxInput(tc.action)
			in := spec.Example
			config := in["configuration"].(map[string]any)
			config[tc.key] = tc.value
			if tc.action == "config edit" && tc.key == "ThumbList" {
				config["Open"] = true
			}
			raw, _ := json.Marshal(in)
			r, e := RunBlackbox(context.Background(), tc.action, Request{Input: raw, Plan: true})
			if e == nil || r.Complete {
				t.Fatalf("invalid supplied field accepted: %+v %v", r, e)
			}
		})
	}
}
