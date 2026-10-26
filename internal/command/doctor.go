package command

import "errors"

func validateDoctorOptions(opts Options) error {
	if opts.session != "" && (opts.installation != "" || opts.pid != 0) {
		return errors.New("use --session with its original --project, or select --installation/--pid")
	}
	if !opts.fix && (opts.hardReset || opts.release != "" || opts.output != "" || opts.request != "") {
		return errors.New("--hard-reset, --release, --output and --request require --fix")
	}
	return nil
}
