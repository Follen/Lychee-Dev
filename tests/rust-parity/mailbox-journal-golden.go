// Regenerate with an official Go toolchain:
//   go run tests/rust-parity/mailbox-journal-golden.go
// The event struct and hash function are copied from the fixed oracle:
// 3057a9666ab078270b63c7f06de748069b0d1f4b:internal/live/journal/memory_log.go.
// This standalone, standard-library-only generator does not use Rust output.
package main

import (
    "crypto/sha256"
    "encoding/json"
    "fmt"
    "os"
)

type MemoryEvent struct {
    Sequence int             `json:"sequence"`
    Previous string          `json:"previous"`
    Kind     string          `json:"kind"`
    Data     json.RawMessage `json:"data"`
    Checksum string          `json:"checksum"`
}

func memoryEventHash(event MemoryEvent) string {
    event.Checksum = ""
    b, _ := json.Marshal(event)
    return fmt.Sprintf("%x", sha256.Sum256(b))
}

func main() {
    golden := struct {
        OracleCommit string   `json:"oracleCommit"`
        Source       string   `json:"source"`
        Preimages    []string `json:"preimages"`
        Checksums    []string `json:"checksums"`
        Lines        []string `json:"lines"`
        Chain        string   `json:"chain"`
    }{
        OracleCommit: "3057a9666ab078270b63c7f06de748069b0d1f4b",
        Source: "internal/live/journal/memory_log.go:MemoryEvent,memoryEventHash",
    }
    inputs := []struct {kind, data string}{
        {"connect<&>\u2028\u2029", " { \"z\": 1.00, \"a\": \"<&>\u2028\u2029\", \"escaped\":\"\\u003c\" } "},
        {"budget_observed", " { \"n\": -0.0, \"nested\": { \"b\": 1e+02, \"a\": \"<&>\" } } "},
    }
    previous := ""
    for i, input := range inputs {
        event := MemoryEvent{Sequence: i+1, Previous: previous, Kind: input.kind, Data: json.RawMessage(input.data)}
        preimage, err := json.Marshal(event)
        if err != nil { panic(err) }
        event.Checksum = memoryEventHash(event)
        line, err := json.Marshal(event)
        if err != nil { panic(err) }
        golden.Preimages = append(golden.Preimages, string(preimage))
        golden.Checksums = append(golden.Checksums, event.Checksum)
        golden.Lines = append(golden.Lines, string(line)+"\n")
        golden.Chain += string(line)+"\n"
        previous = event.Checksum
    }
    encoder := json.NewEncoder(os.Stdout)
    encoder.SetIndent("", "  ")
    if err := encoder.Encode(golden); err != nil { panic(err) }
}
