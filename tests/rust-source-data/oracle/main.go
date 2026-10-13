// Test-only frozen-Go differential adapter. Rust production never calls it.
package main

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
	"os"
	"path/filepath"
	"time"
	"unicode"
)

func check(err error) {
	if err != nil {
		panic(err)
	}
}
func main() {
	if len(os.Args)==3 && os.Args[1]=="matrix" {
		var cases []struct {Path string `json:"path"`; Targets []codebase.TOCValidation `json:"targets"`}
		input,err:=os.ReadFile(os.Args[2]);check(err);check(json.Unmarshal(input,&cases));outputs:=make([]any,0,len(cases))
		for _,c:=range cases{outputs=append(outputs,codebase.MergeMatrix(c.Path,c.Targets))};check(json.NewEncoder(os.Stdout).Encode(outputs));return
	}
	if len(os.Args)==3 && os.Args[1]=="relational" {
		var cases []struct {Name string `json:"name"`;SQL string `json:"sql"`;Sources map[string]struct{Columns []string `json:"columns"`;Rows [][]any `json:"rows"`} `json:"sources"`;Parameters map[string]any `json:"parameters"`}
		input,err:=os.ReadFile(os.Args[2]);check(err);decoder:=json.NewDecoder(bytes.NewReader(input));decoder.UseNumber();check(decoder.Decode(&cases))
		outputs:=make([]any,0,len(cases));for _,c:=range cases{
			ctx:=context.Background();p,err:=relational.Compile(ctx,c.SQL);var result any
			if err==nil{r,e:=p.Execute(ctx,func(ctx context.Context,use relational.TableUse)(relational.Source,error){s,ok:=c.Sources[use.Name];if !ok{return relational.Source{},relational.ErrBinding};return relational.Source{Columns:s.Columns,Scan:func(ctx context.Context,yield func([]any)error)error{for _,row:=range s.Rows{if err:=ctx.Err();err!=nil{return err};if err:=yield(row);err!=nil{return err}};return nil}},nil},c.Parameters,relational.Limits{Work:1000000,MemoryBytes:64<<20});err=e;result=r}
			if err!=nil{code:="unknown";for _,known:=range []error{relational.ErrSyntax,relational.ErrBinding,relational.ErrType,relational.ErrBudget,relational.ErrUnsupported,relational.ErrNumericRange}{if errors.Is(err,known){code=known.Error();break}};outputs=append(outputs,map[string]any{"name":c.Name,"error":code})}else{outputs=append(outputs,map[string]any{"name":c.Name,"result":result})}
		};check(json.NewEncoder(os.Stdout).Encode(outputs));return
	}
	if len(os.Args) == 3 && os.Args[1] == "environment" {
		var cases []struct {
			Name string `json:"name"`
			Documents map[string]string `json:"documents"`
			Identity *environment.Identity `json:"identity"`
		}
		input, err := os.ReadFile(os.Args[2]); check(err)
		check(json.Unmarshal(input, &cases))
		outputs := make([]any, 0, len(cases))
		for _, c := range cases {
			docs := make(map[string][]byte)
			for path, raw := range c.Documents { docs[path] = []byte(raw) }
			identity := environment.Identity{Repository:"Gethe/wow-ui-source",Commit:"31c7f7b9cc79e56c986b365c06a6afbcf3c9177b",Client:"retail",GeneratorVersion:"environment-ts-v1"}
			if c.Identity != nil { identity = *c.Identity }
			result, err := environment.Build(context.Background(), identity, docs, environment.Limits{}); check(err)
			outputs = append(outputs, map[string]any{"name":c.Name,"result":map[string]any{"manifest":result.Manifest,"definitions":string(result.Definitions),"facts":result.Facts,"mappings":result.Mappings,"coverage":result.Coverage}})
		}
		check(json.NewEncoder(os.Stdout).Encode(outputs)); return
	}
	if len(os.Args) == 2 && os.Args[1] == "unicode-simple" {
		rows := make([][3]int32, 0, 4096)
		for cp := rune(0); cp <= unicode.MaxRune; cp++ {
			if cp >= 0xD800 && cp <= 0xDFFF {
				continue
			}
			upper, lower := unicode.ToUpper(cp), unicode.ToLower(cp)
			if upper != cp || lower != cp {
				rows = append(rows, [3]int32{cp, upper, lower})
			}
		}
		check(json.NewEncoder(os.Stdout).Encode(struct {
			Version string     `json:"version"`
			Rows    [][3]int32 `json:"rows"`
		}{unicode.Version, rows}))
		return
	}
	if len(os.Args) != 4 {
		panic("commonRoot exactBuild definitionsDir")
	}
	start := time.Now()
	ctx := context.Background()
	root, build, dir := os.Args[1], os.Args[2], os.Args[3]
	meta, err := records.ResolveLocalBuild(ctx, root, "wow", build)
	check(err)
	encoding, err := records.OpenLocalObject(ctx, root, meta.EncodingKey, meta.EncodingBytes)
	check(err)
	defer encoding.Close()
	limits := container.Limits{EncodedBytes: 256 << 20, DecodedBytes: 256 << 20, ChunkBytes: 64 << 20, Chunks: 65536, Depth: 16}
	ranges, err := container.OpenRanges(ctx, encoding, meta.EncodingBytes, limits, nil)
	check(err)
	index, err := records.OpenEncoding(ctx, ranges)
	check(err)
	mapping, err := index.FindContent(ctx, meta.RootContentKey)
	check(err)
	physical, err := index.FindEncoding(ctx, mapping.EncodingKeys[0])
	check(err)
	rootObject, err := records.OpenLocalObject(ctx, root, physical.EncodingKey, physical.EncodedBytes)
	check(err)
	defer rootObject.Close()
	var rootBytes bytes.Buffer
	_, err = container.Decode(ctx, &rootBytes, rootObject, limits)
	check(err)
	rootSum := md5.Sum(rootBytes.Bytes())
	if hex.EncodeToString(rootSum[:]) != mapping.ContentKey {
		panic("root CKey mismatch")
	}
	entries, err := records.LookupRoot(ctx, bytes.NewReader(rootBytes.Bytes()), int64(rootBytes.Len()), []uint32{1349477}, records.RootLimits{Bytes: 256 << 20, Records: 10000000, Groups: 1 << 20, Matches: 1 << 20})
	check(err)
	var selected []records.RootRecord
	for _, entry := range entries {
		if entry.LocaleMask&0x40 != 0 && entry.ContentFlags&0x80 == 0 {
			selected = append(selected, entry)
		}
	}
	if len(selected) != 1 {
		panic("zhCN standard file identity ambiguous")
	}
	fileMapping, err := index.FindContent(ctx, selected[0].ContentKey)
	check(err)
	filePhysical, err := index.FindEncoding(ctx, fileMapping.EncodingKeys[0])
	check(err)
	fileObject, err := records.OpenLocalObject(ctx, root, filePhysical.EncodingKey, filePhysical.EncodedBytes)
	check(err)
	defer fileObject.Close()
	var fileBytes bytes.Buffer
	_, err = container.Decode(ctx, &fileBytes, fileObject, limits)
	check(err)
	raw := fileBytes.Bytes()
	fileSum := md5.Sum(raw)
	if hex.EncodeToString(fileSum[:]) != fileMapping.ContentKey {
		panic("file CKey mismatch")
	}
	sha := sha256.Sum256(raw)
	manifestRaw, err := os.ReadFile(filepath.Join(dir, "manifest.json"))
	check(err)
	manifest, err := schema.ParseManifest(ctx, manifestRaw)
	check(err)
	budget := table.Budget{FileBytes: 256 << 20, MetadataBytes: 64 << 20, Rows: 10000000, Columns: 65536, Partitions: 65536}
	layout, err := table.Inspect(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	check(err)
	identity, err := manifest.Resolve(ctx, "Map", 1349477, layout.TableHash)
	check(err)
	definitionRaw, err := os.ReadFile(filepath.Join(dir, identity.Name+".dbd"))
	check(err)
	doc, err := schema.Parse(ctx, definitionRaw)
	check(err)
	rows, err := table.OpenRecords(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	check(err)
	view, err := rows.Bind(ctx, doc, build)
	check(err)
	row, err := view.Row(ctx, 0, 1<<20)
	check(err)
	var allRows []map[string]any
	check(view.Scan(ctx, 4<<20, 1<<20, func(row map[string]any) error { allRows = append(allRows, row); return nil }))
	source, err := records.TableSource(view, 4<<20)
	check(err)
	var queries []map[string]any
	for _, sql := range []string{"SELECT ID FROM Map WHERE ID>=0 ORDER BY ID", "SELECT ID, Directory FROM Map WHERE ID BETWEEN 0 AND 20 ORDER BY ID DESC LIMIT 5 OFFSET 1", "SELECT ID, \"Flags[0]\" FROM Map WHERE ID=0", "SELECT ID+1 AS NextID FROM Map WHERE ID IN (0,1,2) ORDER BY NextID DESC", "SELECT DISTINCT InstanceType FROM Map ORDER BY InstanceType", "SELECT ID, MapName_lang FROM Map WHERE ID=0 AND LENGTH(MapName_lang)>0"} {
		p, err := relational.Compile(ctx, sql)
		check(err)
		r, err := p.Execute(ctx, func(_ context.Context, use relational.TableUse) (relational.Source, error) {
			if use.Name != "Map" {
				return relational.Source{}, relational.ErrBinding
			}
			return source, nil
		}, nil, relational.Limits{Work: 5000000, MemoryBytes: 64 << 20})
		check(err)
		queries = append(queries, map[string]any{"sql": sql, "columns": r.Columns, "rows": r.Rows})
	}
	sql := "SELECT ID, Directory, MapName_lang FROM Map WHERE ID=0"
	program, err := relational.Compile(ctx, sql)
	check(err)
	result, err := program.Execute(ctx, func(_ context.Context, use relational.TableUse) (relational.Source, error) {
		if use.Name != "Map" {
			return relational.Source{}, relational.ErrBinding
		}
		return source, nil
	}, nil, relational.Limits{Work: 5000000, MemoryBytes: 64 << 20})
	check(err)
	output := map[string]any{"schema": "lycheedev.frozen-go-local-data-proof.v1", "metadata": meta, "rootMapping": mapping, "rootEncoding": physical, "entries": entries, "decodedSHA256": hex.EncodeToString(sha[:]), "row": row, "allRows": allRows, "queries": queries, "sql": sql, "sqlResult": result, "elapsedMs": float64(time.Since(start).Nanoseconds()) / 1e6, "boundary": "Independent real CASC/config/Encoding/Root/file CKey, manifest/layout/DBD/typed decoding/SQL; no Rust export dependency"}
	encoded, err := json.Marshal(output)
	check(err)
	fmt.Println(string(encoded))
}
