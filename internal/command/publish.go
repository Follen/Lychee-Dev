package command

import (
	"context"
	"io"
	"os"
	"strings"

	"github.com/follenfang/lycheedev/internal/publish"
)

func runPublishCommand(ctx context.Context, route string, opts Options, response *Envelope) (int, error) {
	platform, action, _ := strings.Cut(strings.TrimPrefix(route, "publish "), " ")
	if opts.file != "" && opts.stdin {
		return 2, &publish.Error{Code: "publish.input", Path: "$input", Message: "choose --file or --stdin", Exit: 2}
	}
	if publishRequestRequired(route) && opts.request == "" {
		return 2, &publish.Error{Code: "publish.input", Path: "$request", Message: "this operation requires --request", Exit: 2}
	}
	raw := []byte("{}")
	var err error
	if opts.stdin {
		raw, err = readBoundedStdin(ctx, os.Stdin, publish.MaxInputBytes)
	} else if opts.file != "" {
		var file *os.File
		file, err = os.Open(opts.file)
		if err == nil {
			raw, err = io.ReadAll(io.LimitReader(file, publish.MaxInputBytes+1))
			closeErr := file.Close()
			if err == nil {
				err = closeErr
			}
		}
	}
	if err != nil {
		return 0, err
	}
	home, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	request := publish.Request{Home: home, RequestID: opts.request, Input: raw, Plan: opts.plan}
	var result publish.Result
	switch platform {
	case "curseforge":
		result, err = publish.RunCurseForge(ctx, action, request)
	case "dd":
		result, err = publish.RunDD(ctx, action, request)
	case "newbee":
		result, err = publish.RunNewBee(ctx, action, request)
	case "modus":
		result, err = publish.RunModus(ctx, action, request)
	case "blackbox":
		result, err = publish.RunBlackbox(ctx, action, request)
	default:
		return 2, &publish.Error{Code: "publish.input", Path: "$platform", Message: "unknown publish platform", Exit: 2}
	}
	response.Result = result
	response.OperationID = result.OperationID
	if err != nil {
		return 0, err
	}
	// These commands finish by reading the saved operation. Its own completion
	// and submission state remain in the result, even when the read succeeds.
	switch route {
	case "publish curseforge plugin status", "publish dd operation status",
		"publish newbee operation status", "publish modus operation status",
		"publish modus backup status", "publish blackbox operation status":
		return 0, nil
	}
	if !result.Complete {
		return 6, &publish.Error{Code: "publish.pending", Message: "retain the original operation and use its platform status or verification command", Exit: 6}
	}
	return 0, nil
}

func publishInputContract(route string) (publish.InputSpec, bool) {
	platform, action, _ := strings.Cut(strings.TrimPrefix(route, "publish "), " ")
	switch platform {
	case "curseforge":
		return publish.CurseForgeInput(action)
	case "dd":
		return publish.DDInput(action)
	case "newbee":
		return publish.NewBeeInput(action)
	case "modus":
		return publish.ModusInput(action)
	case "blackbox":
		return publish.BlackboxInput(action)
	}
	return publish.InputSpec{}, false
}
func publishMutates(platform, action string) bool {
	switch platform {
	case "newbee":
		return publish.NewBeeMutates(action)
	case "modus":
		return publish.ModusMutates(action)
	case "blackbox":
		return publish.BlackboxMutates(action)
	case "curseforge":
		return action == "plugin upload" || action == "plugin verify"
	case "dd":
		_, verb, _ := strings.Cut(action, " ")
		switch verb {
		case "start", "close", "create", "update", "edit", "delete", "delete-many", "upload", "rename", "generate", "resume":
			return true
		}
	}
	return false
}
func publishRequestRequired(route string) bool {
	if !strings.HasPrefix(route, "publish ") {
		return false
	}
	platform, action, _ := strings.Cut(strings.TrimPrefix(route, "publish "), " ")
	switch platform {
	case "curseforge":
		return action == "plugin upload"
	case "newbee":
		return publish.NewBeeMutates(action) || strings.HasSuffix(action, " verify") || action == "operation status"
	case "dd":
		if action == "plugin upload" || action == "config upload" || action == "wa upload" {
			return false
		}
		return publishMutates(platform, action) && !strings.HasPrefix(action, "session ") && !strings.HasPrefix(action, "operation ") && action != "backup generate"
	case "modus":
		return publish.ModusMutates(action) && !strings.HasPrefix(action, "operation ") && action != "backup generate" && action != "backup resume"
	case "blackbox":
		return publish.BlackboxMutates(action) && !strings.HasPrefix(action, "operation ") && !strings.HasPrefix(action, "session ") && action != "backup generate"
	}
	return false
}
