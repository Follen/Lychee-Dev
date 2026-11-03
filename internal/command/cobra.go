package command

import (
	"context"
	"errors"
	"fmt"
	"io"
	"strings"

	"github.com/spf13/cobra"
)

type inputFieldError struct {
	Field   string `json:"field"`
	Command string `json:"command"`
	Cause   error  `json:"-"`
}

func (e *inputFieldError) Error() string { return e.Cause.Error() }
func (e *inputFieldError) Unwrap() error { return e.Cause }

type parsedFlag struct {
	name, value string
	boolean     bool
}
type contractValue struct {
	name    string
	boolean bool
	values  *[]parsedFlag
}

func (v *contractValue) String() string { return "" }
func (v *contractValue) Type() string {
	if v.boolean {
		return "bool"
	}
	return "value"
}
func (v *contractValue) Set(value string) error {
	*v.values = append(*v.values, parsedFlag{v.name, value, v.boolean})
	return nil
}

// Each execution owns its command tree and flag state. Cobra performs token,
// command and -- parsing; the contract binds and validates business values.
func parseCommand(ctx context.Context, args []string) (Options, error) {
	opts := Options{format: "text", line: 1, count: 80, limit: 50, maxBytes: 128 << 20, sourceDepth: 1, sourceMaxLines: 120}
	// Recover output encoding even when an earlier unknown flag stops Cobra.
	// Respect value-bearing flags so SQL/text/path values cannot become options.
	valueFlags := map[string]bool{"--format": true, "--home": true}
	for _, c := range commandContracts {
		for _, display := range c.acceptedFlagDisplays() {
			spec, _ := c.flag(strings.Fields(display)[0])
			valueFlags[spec.name] = spec.value
		}
	}
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if arg == "--" {
			break
		}
		name, value, inline := strings.Cut(arg, "=")
		if valueFlags[name] && !inline && i+1 < len(args) {
			i++
			value = args[i]
		}
		if name == "--format" {
			opts.format = value
		}
	}
	if opts.format != "text" && opts.format != "json" && opts.format != "jsonl" {
		opts.format = "json"
		return opts, errors.New("format must be text, json, or jsonl")
	}
	var values []parsedFlag
	root := &cobra.Command{Use: "lycheedev", SilenceUsage: true, SilenceErrors: true, CompletionOptions: cobra.CompletionOptions{DisableDefaultCmd: true}}
	root.SetOut(io.Discard)
	root.SetErr(io.Discard)
	root.SetFlagErrorFunc(func(cmd *cobra.Command, err error) error {
		message := strings.Replace(err.Error(), "unknown flag: ", "unknown flag ", 1)
		return errors.New(message)
	})
	root.SetArgs(args)
	// A union at the root preserves flags preceding a command. Admissibility is
	// checked against only the selected leaf, never this union.
	registered := map[string]bool{}
	register := func(spec flagSpec) {
		if registered[spec.name] {
			return
		}
		registered[spec.name] = true
		name := strings.TrimPrefix(spec.name, "--")
		root.PersistentFlags().Var(&contractValue{spec.name, !spec.value, &values}, name, spec.display)
		if !spec.value {
			root.PersistentFlags().Lookup(name).NoOptDefVal = "true"
		}
	}
	register(flagSpec{name: "--format", value: true, display: "--format text|json|jsonl"})
	register(flagSpec{name: "--home", value: true, display: "--home <root>"})
	for _, c := range commandContracts {
		for _, display := range c.acceptedFlagDisplays() {
			name := strings.Fields(display)[0]
			if spec, ok := c.flag(name); ok {
				register(spec)
			}
		}
	}
	root.PersistentFlags().BoolP("help", "h", false, "Show command contract")
	nodes := map[string]*cobra.Command{"": root}
	finish := func(cmd *cobra.Command, positionals []string, help bool) error {
		route := strings.TrimPrefix(cmd.CommandPath(), "lycheedev")
		route = strings.TrimSpace(route)
		opts.words = append(canonicalCommandWords(strings.Fields(route)), positionals...)
		route = strings.Join(canonicalCommandWords(strings.Fields(route)), " ")
		opts.help = help || route == ""
		contract, _, err := findCommandContract(opts.words)
		if err != nil {
			if !help || !cmd.HasSubCommands() {
				return err
			}
			contract = nil
		}
		if route == "asset search" {
			opts.maxBytes = 256 << 20
		}
		if route == "source context" {
			opts.maxBytes = 16 << 10
		}
		if !opts.help && contract != nil && contract.positional != "" && len(positionals) == 0 {
			return fmt.Errorf("missing required argument %s", contract.positional)
		}
		seen := map[string]bool{}
		for _, v := range values {
			if _, ok := commandFlag(contract, v.name); !ok {
				return &inputFieldError{v.name, route, unsupportedRouteFlag(route, v.name)}
			}
			if seen[v.name] && v.name != "--param" && !(route == "update" && (v.name == "--path" || v.name == "--installation")) {
				return &inputFieldError{v.name, route, fmt.Errorf("repeated flag %s", v.name)}
			}
			seen[v.name] = true
			if err := bindOption(&opts, route, v.name, v.value, v.boolean); err != nil {
				return &inputFieldError{v.name, route, err}
			}
		}
		return validateOptions(opts, route, seen)
	}
	root.RunE = func(cmd *cobra.Command, args []string) error { return finish(cmd, args, false) }
	for _, c := range commandContracts {
		for _, path := range append([]string{c.Path}, c.commandAliases()...) {
			parts := strings.Fields(path)
			for i, part := range parts {
				key := strings.Join(parts[:i+1], " ")
				if nodes[key] != nil {
					continue
				}
				node := &cobra.Command{Use: part, SilenceUsage: true, SilenceErrors: true}
				node.RunE = func(cmd *cobra.Command, args []string) error { return finish(cmd, args, false) }
				nodes[strings.Join(parts[:i], " ")].AddCommand(node)
				nodes[key] = node
			}
			nodes[path].Short = c.Summary
		}
	}

	var helpErr error
	root.SetHelpFunc(func(cmd *cobra.Command, args []string) { helpErr = finish(cmd, cmd.Flags().Args(), true) })
	executed, err := root.ExecuteContextC(ctx)
	if err != nil && len(opts.words) == 0 && executed != nil {
		opts.words = canonicalCommandWords(strings.Fields(strings.TrimSpace(strings.TrimPrefix(executed.CommandPath(), "lycheedev"))))
	}
	if err == nil {
		err = helpErr
	}
	return opts, err
}
