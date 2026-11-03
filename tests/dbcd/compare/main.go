// Command compare performs lossless JSON comparison of independent reader
// output. It validates the pinned oracle before comparing its evidence.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"math/big"
	"os"
	"reflect"
)

func main() {
	if len(os.Args) != 3 {
		panic("expected oracle.json lychee.json")
	}
	a, err := read(os.Args[1])
	if err != nil {
		panic(err)
	}
	b, err := read(os.Args[2])
	if err != nil {
		panic(err)
	}
	if err := compare(a, b); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("matched count=%v, complete ID/field digests and blocks, encrypted IDs and %d sampled rows\n", a["count"], len(a["rows"].([]any)))
}
func compare(a, b map[string]any) error {
	if a["sourceCommit"] != "c0aeb5f6be0e9fbffc8eadb4f8b9b4e36b42af7c" {
		return fmt.Errorf("unexpected oracle sourceCommit: %v", a["sourceCommit"])
	}
	for _, key := range []string{"db2SHA256", "dbdSHA256", "fieldDigest", "fieldBlocks", "hotfixSHA256", "build"} {
		if _, ok := a[key]; !ok {
			return fmt.Errorf("oracle missing %s", key)
		}
	}
	checked := make(map[string]any, len(a)-1)
	for key, value := range a {
		if key != "sourceCommit" {
			checked[key] = value
		}
	}
	return equal("$", checked, b)
}
func read(path string) (map[string]any, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	d := json.NewDecoder(f)
	d.UseNumber()
	var value map[string]any
	if err := d.Decode(&value); err != nil {
		return nil, err
	}
	if _, err := d.Token(); err != io.EOF {
		return nil, fmt.Errorf("trailing JSON")
	}
	return value, nil
}
func equal(path string, a, b any) error {
	fail := func() error { return fmt.Errorf("%s differs: %v versus %v", path, a, b) }
	switch x := a.(type) {
	case json.Number:
		y, ok := b.(json.Number)
		if !ok {
			return fail()
		}
		p, po := new(big.Rat).SetString(string(x))
		q, qo := new(big.Rat).SetString(string(y))
		if !po || !qo || p.Cmp(q) != 0 {
			return fail()
		}
	case map[string]any:
		y, ok := b.(map[string]any)
		if !ok || len(x) != len(y) {
			return fail()
		}
		for k, v := range x {
			other, ok := y[k]
			if !ok {
				return fmt.Errorf("%s.%s missing", path, k)
			}
			if err := equal(path+"."+k, v, other); err != nil {
				return err
			}
		}
	case []any:
		y, ok := b.([]any)
		if !ok || len(x) != len(y) {
			return fail()
		}
		for i, v := range x {
			if err := equal(fmt.Sprintf("%s[%d]", path, i), v, y[i]); err != nil {
				return err
			}
		}
	default:
		if !reflect.DeepEqual(a, b) {
			return fail()
		}
	}
	return nil
}
