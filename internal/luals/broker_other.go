//go:build !windows

package luals

import (
	"context"
	"net"
)

const BrokerWire = "lycheedev.luals-broker.v1"

func BrokerScope(string) (string, string, string, error)       { return "", "", "", ErrUnavailable }
func ListenBrokerPipe(string, string) (net.Listener, error)    { return nil, ErrUnavailable }
func DialBrokerPipe(context.Context, string) (net.Conn, error) { return nil, ErrUnavailable }
func VerifyBrokerPeer(net.Conn, uint32, uint64, string) error  { return ErrUnavailable }
func BrokerProcessIdentity(uint32) (uint64, string, error)     { return 0, "", ErrUnavailable }
func StartBroker(string, []string) error                       { return ErrUnavailable }
func OwnBrokerJob() (func(), error)                            { return nil, ErrUnavailable }
