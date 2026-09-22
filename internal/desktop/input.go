package desktop

import (
	"context"
	"errors"
	"strings"
	"unicode/utf16"
	"unicode/utf8"
)

// InputReceipt reports queue submission only, never game execution.
type InputReceipt struct {
	MessagesQueued     int  `json:"messagesQueued"`
	SubmissionComplete bool `json:"submissionComplete"`
}

const bootstrapIdentifyPrefix = "/dev bridge identify "
const bootstrapConnectCommand = "/dev connect"

// QueueBootstrapCommand is one narrow, fixed exception used only to obtain the
// first verifiable receipt: it may send exactly /dev bridge identify <32-hex>
// or /dev connect and nothing else. It keeps every safety property of
// QueueCommand. Once a verified receipt exists, QueuePreparedCommand remains
// the only way to send anything else.
func QueueBootstrapCommand(parent context.Context, target WindowIdentity, command string) (InputReceipt, error) {
	if err := bootstrapCommand(command); err != nil {
		return InputReceipt{}, err
	}
	if _, err := commandUnits(command); err != nil {
		return InputReceipt{}, err
	}
	return queueBootstrapCommand(parent, target, command)
}

// bootstrapCommand is the complete allowlist of the bootstrap entry. Any other
// text is a hard error, not a fallback to the general command channel.
func bootstrapCommand(command string) error {
	if command == bootstrapConnectCommand {
		return nil
	}
	if strings.HasPrefix(command, bootstrapIdentifyPrefix) {
		nonce := command[len(bootstrapIdentifyPrefix):]
		if len(nonce) == 32 {
			for _, c := range nonce {
				if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
					return errors.New("desktop.bootstrap_command_not_allowed")
				}
			}
			return nil
		}
	}
	return errors.New("desktop.bootstrap_command_not_allowed")
}

func commandUnits(command string) ([]uint16, error) {
	if !utf8.ValidString(command) || len(command) == 0 || len(command) > 255 || command[0] != '/' {
		return nil, errors.New("desktop.invalid_command")
	}
	for _, r := range command {
		if r < 32 || r == 127 {
			return nil, errors.New("desktop.command_control_character")
		}
	}
	return utf16.Encode([]rune(command)), nil
}
