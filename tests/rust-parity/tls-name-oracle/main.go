// The test certificates and key are public, test-only fixtures. This executable
// uses an in-memory RootCAs pool, never the system trust store. Only loopback is
// dialed. It is a standard-library observation oracle, not production transport.
package main

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"time"
)

type observation struct {
	Input       string `json:"input"`
	SNI         string `json:"sni"`
	Accepted    bool   `json:"accepted"`
	Phase       string `json:"phase"`
	Error       string `json:"error,omitempty"`
	ServerError string `json:"serverError,omitempty"`
	GoVersion   string `json:"goVersion"`
	GOOS        string `json:"goos"`
	Trust       string `json:"trust"`
}
type serverResult struct {
	sni string
	err error
}

func observe(name string, identity tls.Certificate, roots *x509.CertPool) observation {
	out := observation{Input: name, GoVersion: runtime.Version(), GOOS: runtime.GOOS, Trust: "private test root only"}
	listener, err := net.ListenTCP("tcp4", &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		out.Phase = "listen"
		out.Error = err.Error()
		return out
	}
	deadline := time.Now().Add(5 * time.Second)
	if err := listener.SetDeadline(deadline); err != nil {
		listener.Close()
		out.Phase = "listen-deadline"
		out.Error = err.Error()
		return out
	}
	done := make(chan serverResult, 1)
	go func() {
		result := serverResult{}
		defer func() { listener.Close(); done <- result }()
		socket, err := listener.AcceptTCP()
		if err != nil {
			result.err = err
			return
		}
		defer socket.Close()
		if err := socket.SetDeadline(deadline); err != nil {
			result.err = err
			return
		}
		config := &tls.Config{Certificates: []tls.Certificate{identity}}
		config.GetConfigForClient = func(hello *tls.ClientHelloInfo) (*tls.Config, error) {
			result.sni = hello.ServerName
			return nil, nil
		}
		stream := tls.Server(socket, config)
		defer stream.Close()
		ctx, cancel := context.WithDeadline(context.Background(), deadline)
		defer cancel()
		if err := stream.HandshakeContext(ctx); err != nil {
			result.err = err
			return
		}
		_, result.err = stream.Write([]byte("ok"))
	}()
	ctx, cancel := context.WithDeadline(context.Background(), deadline)
	defer cancel()
	socket, err := (&net.Dialer{}).DialContext(ctx, "tcp4", listener.Addr().String())
	out.Phase = "connect"
	if err == nil {
		if err = socket.SetDeadline(deadline); err != nil {
			out.Phase = "connect-deadline"
			socket.Close()
			listener.Close()
			server := <-done
			out.SNI = server.sni
			out.Error = err.Error()
			if server.err != nil {
				out.ServerError = server.err.Error()
			}
			return out
		}
		stream := tls.Client(socket, &tls.Config{RootCAs: roots, ServerName: name})
		out.Phase = "tls"
		err = stream.HandshakeContext(ctx)
		if err == nil {
			out.Phase = "reply"
			var reply [2]byte
			_, err = io.ReadFull(stream, reply[:])
			if err == nil && string(reply[:]) != "ok" {
				err = errors.New("wrong fixture reply")
			}
		}
		stream.Close()
		socket.Close()
	}
	// The server's accept/read/write deadline bounds this join on every branch.
	server := <-done
	out.SNI = server.sni
	if server.err != nil {
		out.ServerError = server.err.Error()
	}
	if err != nil {
		out.Error = err.Error()
	} else {
		out.Accepted = true
		out.Phase = "complete"
	}
	return out
}

func main() {
	fixtures := flag.String("fixtures", "", "directory containing public test-only root.pem/server.pem/server-key.pem")
	flag.Parse()
	if *fixtures == "" {
		fmt.Fprintln(os.Stderr, "-fixtures is required")
		os.Exit(2)
	}
	raw, err := os.ReadFile(filepath.Join(*fixtures, "root.pem"))
	if err != nil {
		panic(err)
	}
	roots := x509.NewCertPool()
	if !roots.AppendCertsFromPEM(raw) {
		panic("invalid fixture root")
	}
	identity, err := tls.LoadX509KeyPair(filepath.Join(*fixtures, "server.pem"), filepath.Join(*fixtures, "server-key.pem"))
	if err != nil {
		panic(err)
	}
	var rows []observation
	for _, name := range []string{"example.test", "example.test.", "example.test..", "example.test...", "wrong.test"} {
		rows = append(rows, observe(name, identity, roots))
	}
	if err := json.NewEncoder(os.Stdout).Encode(rows); err != nil {
		panic(err)
	}
	for _, v := range rows {
		expected := v.Input == "example.test" || v.Input == "example.test."
		expectedSNI := "example.test"
		if v.Input == "wrong.test" {
			expectedSNI = v.Input
		}
		if v.Accepted != expected || v.SNI != expectedSNI || (!expected && v.Phase != "tls") {
			fmt.Fprintln(os.Stderr, "unexpected fixed-Go private-root fixture result for", v.Input)
			os.Exit(1)
		}
	}
}
