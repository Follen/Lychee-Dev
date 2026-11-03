//go:build windows && amd64

package publish

import (
	"golang.org/x/sys/windows"
	"reflect"
	"runtime"
	"testing"
	"unsafe"
)

func TestDDAppIDABIReleasesObjectsAndBothBuffers(t *testing.T) {
	for _, scenario := range []string{"success", "init-error", "perform-error", "ambiguous-json"} {
		t.Run(scenario, func(t *testing.T) {
			events := []string{}
			raw := append([]byte(`{"appId":"synthetic-app-id"}`), 0)
			if scenario == "ambiguous-json" {
				raw = append([]byte(`{"appId":"one","appId":"two"}`), 0)
			}
			message := append([]byte("synthetic-private-error"), 0)
			var sdk, method struct{ table *uintptr }
			sdkTable := [6]uintptr{}
			methodTable := [4]uintptr{}
			sdk.table = &sdkTable[0]
			method.table = &methodTable[0]
			sdkTable[2] = windows.NewCallback(func(self uintptr) uintptr { events = append(events, "sdk-release"); return 0 })
			sdkTable[3] = windows.NewCallback(func(self, buffer uintptr) uintptr {
				switch buffer {
				case uintptr(unsafe.Pointer(&raw[0])):
					events = append(events, "json-free")
				case uintptr(unsafe.Pointer(&message[0])):
					events = append(events, "message-free")
				default:
					t.Error("unknown native buffer")
				}
				return 0
			})
			sdkTable[4] = windows.NewCallback(func(self uintptr, id int32) uintptr {
				events = append(events, "method-create")
				if id != 2 {
					t.Error("wrong SDK method")
				}
				return uintptr(unsafe.Pointer(&method))
			})
			sdkTable[5] = windows.NewCallback(func(self uintptr, params *[6]*byte) uintptr {
				events = append(events, "init")
				for i, want := range []string{"cc_team", "1.1.30", "public-product", "one", "two", "three"} {
					if windows.BytePtrToString(params[i]) != want {
						t.Error("incorrect SDK argument")
					}
				}
				if scenario == "init-error" {
					return 9
				}
				return 0
			})
			methodTable[1] = windows.NewCallback(func(self uintptr) uintptr { events = append(events, "method-release"); return 0 })
			methodTable[3] = windows.NewCallback(func(self uintptr, out, messageOut **byte) uintptr {
				events = append(events, "perform")
				*out = &raw[0]
				*messageOut = &message[0]
				if scenario == "perform-error" {
					return 9
				}
				return 0
			})
			factory := windows.NewCallback(func() uintptr { events = append(events, "factory"); return uintptr(unsafe.Pointer(&sdk)) })
			value, err := ddInvokeAppID(factory, []string{"cc_team", "1.1.30", "public-product", "one", "two", "three"})
			runtime.KeepAlive(sdk)
			runtime.KeepAlive(method)
			runtime.KeepAlive(sdkTable)
			runtime.KeepAlive(methodTable)
			runtime.KeepAlive(raw)
			runtime.KeepAlive(message)
			if scenario == "success" {
				if err != nil || value != "synthetic-app-id" {
					t.Fatalf("%s %v", value, err)
				}
			} else if err == nil {
				t.Fatal("invalid SDK output accepted")
			}
			want := []string{"factory", "init", "method-create", "perform", "message-free", "json-free", "method-release", "sdk-release"}
			if scenario == "init-error" {
				want = []string{"factory", "init", "sdk-release"}
			}
			if !reflect.DeepEqual(events, want) {
				t.Fatalf("lifetime %v", events)
			}
		})
	}
}
