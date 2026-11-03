package publish

import (
	"context"
	"sort"
	"strings"
)

// NewBeeActions lists the explicit channels; choosing one never falls back to another.
func NewBeeActions() []string {
	actions := make([]string, 0, len(newBeeOfficialCommands))
	for action := range newBeeOfficialCommands {
		actions = append(actions, "official "+action)
	}
	for action := range newBeeCreatorCommands {
		actions = append(actions, "creator "+action)
		if newBeeCreatorCommands[action].write && (!strings.HasPrefix(action, "backup ") || action == "backup rename") {
			actions = append(actions, "creator "+action+" verify")
		}
	}
	actions = append(actions, "operation status")
	sort.Strings(actions)
	return actions
}

func NewBeeMutates(action string) bool {
	if strings.HasPrefix(action, "creator ") {
		return newBeeCreatorCommands[strings.TrimPrefix(action, "creator ")].write
	}
	return newBeeOfficialCommands[strings.TrimPrefix(action, "official ")].write
}

func NewBeeInput(action string) (InputSpec, bool) {
	if action == "operation status" {
		return InputSpec{Properties: map[string]Field{}, Effects: []string{"read the original request receipt without credentials or network"}, Example: map[string]any{}}, true
	}
	if strings.HasPrefix(action, "creator ") {
		key := strings.TrimPrefix(action, "creator ")
		verify := strings.HasSuffix(key, " verify")
		key = strings.TrimSuffix(key, " verify")
		c, ok := newBeeCreatorCommands[key]
		if verify {
			if !c.write || strings.HasPrefix(key, "backup ") && key != "backup rename" {
				return InputSpec{}, false
			}
			c.spec.Effects = []string{"verify the original request by reading the selected resource; never resubmit"}
		}
		if ok {
			c.spec = newBeeExample(c.spec)
		}
		return c.spec, ok
	}
	if c, ok := newBeeOfficialCommands[strings.TrimPrefix(action, "official ")]; ok && strings.HasPrefix(action, "official ") {
		return newBeeExample(c.spec), true
	}
	return InputSpec{}, false
}

func newBeeExample(spec InputSpec) InputSpec {
	var example func(Field) any
	example = func(f Field) any {
		switch f.Type {
		case "string":
			if len(f.Enum) > 0 {
				return f.Enum[0]
			}
			return "example"
		case "integer", "number":
			if f.Minimum != nil {
				return *f.Minimum
			}
			return 1
		case "boolean":
			return true
		case "array":
			if f.Nonempty && f.Items != nil {
				return []any{example(*f.Items)}
			}
			return []any{}
		case "object":
			value := map[string]any{}
			for _, key := range f.Required {
				value[key] = example(f.Properties[key])
			}
			return value
		}
		return nil
	}
	spec.Example = map[string]any{}
	for _, key := range spec.Required {
		spec.Example[key] = example(spec.Properties[key])
	}
	return spec
}

func RunNewBee(ctx context.Context, action string, request Request) (Result, error) {
	result := Result{Platform: "newbee", Action: action}
	spec, ok := NewBeeInput(action)
	if !ok {
		return result, invalid("$action", "unknown NewBee action")
	}
	input, err := spec.Decode(request.Input)
	if err != nil {
		return result, err
	}
	if err = ctx.Err(); err != nil {
		return result, err
	}
	if action == "operation status" {
		if request.RequestID == "" {
			return result, invalid("$request", "operation status requires --request")
		}
		if request.Plan {
			result.Complete = true
			result.Data = map[string]any{"effects": spec.Effects}
			return result, nil
		}
		return newBeeOperationStatus(request)
	}
	if strings.HasPrefix(action, "creator ") {
		key := strings.TrimPrefix(action, "creator ")
		verify := strings.HasSuffix(key, " verify")
		key = strings.TrimSuffix(key, " verify")
		command := newBeeCreatorCommands[key]
		if err := newBeeValidateRelationships(key, input); err != nil {
			return result, err
		}
		if command.write && request.RequestID == "" {
			return result, invalid("$request", "mutating NewBee commands require --request")
		}
		if request.Plan {
			if command.write {
				if _, err := newBeeFingerprint(ctx, "creator "+key, input); err != nil {
					return result, err
				}
			}
			if key == "backup upload" {
				if _, err := newBeeReadBundle(String(input, "bundle_file")); err != nil {
					return result, err
				}
			}
			if strings.HasSuffix(key, " delete") && !Bool(input, "confirm_delete") {
				return result, invalid("$.confirm_delete", "must be true")
			}
			result.Complete = true
			result.Data = map[string]any{"channel": "creator", "effects": spec.Effects, "requiresRemoteValidation": true, "credentialAccess": false, "execution": false}
			return result, nil
		}
		if action == "creator backup scan" {
			value, err := newBeeScan(String(input, "game_dir"))
			result.Data = value
			result.Complete = err == nil
			return result, err
		}
		if action == "creator backup pack" {
			value, err := newBeePack(ctx, input)
			result.Data = value
			result.Complete = err == nil
			return result, err
		}
		client, err := newNewBeeCreator()
		if err != nil {
			return result, err
		}
		if verify {
			return client.verifyOriginal(ctx, key, input, request)
		}
		return client.run(ctx, strings.TrimPrefix(action, "creator "), command, input, request)
	}
	command := newBeeOfficialCommands[strings.TrimPrefix(action, "official ")]
	if _, err = newBeeArgs(command, input); err != nil {
		return result, err
	}
	if command.write && request.RequestID == "" {
		return result, invalid("$request", "mutating NewBee commands require --request")
	}
	if request.Plan {
		if command.write {
			if _, err := newBeeFingerprint(ctx, action, input); err != nil {
				return result, err
			}
		}
		result.Complete = true
		result.Data = map[string]any{"channel": "official", "effects": spec.Effects, "requiresRemoteValidation": command.remote, "credentialAccess": false, "execution": false}
		return result, nil
	}
	return runNewBeeOfficial(ctx, action, command, input, request)
}
