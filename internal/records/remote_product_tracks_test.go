package records

import (
	"context"
	"crypto/md5"
	"fmt"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
)

func TestRemoteTracksKeepCatalogsPinsAndCDNRoutesSeparate(t *testing.T) {
	f := newRemoteTargetFixture(t)
	ctx := context.Background()
	pins := map[string]string{}
	for _, profile := range selection.DataProducts() {
		request := f.request
		request.Product, request.Region = profile.Product, "us"
		build := profile.BuildSeries + ".70338"
		config := []byte(fmt.Sprintf("root = %s\nencoding = %s %s\nencoding-size = 123 456\nbuild-uid = %s\n", strings.Repeat("1", 32), strings.Repeat("2", 32), strings.Repeat("3", 32), profile.ProductCode))
		buildKey := fmt.Sprintf("%x", md5.Sum(config))
		cdnConfig := f.transport.responses[f.configURL].body
		cdnKey := fmt.Sprintf("%x", md5.Sum(cdnConfig))
		base := remoteVersionBase("us") + profile.ProductCode + "/"
		f.transport.responses[base+"versions"] = definitionResponse{status: 200, body: []byte(fmt.Sprintf("Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:4|VersionsName!STRING:0\nus|%s|%s|70338|%s\n", buildKey, cdnKey, build))}
		f.transport.responses[base+"cdns"] = definitionResponse{status: 200, body: []byte("Name!STRING:0|Path!STRING:0|Hosts!STRING:0\nus|tpr/wow|cdn.example.test\n")}
		f.transport.responses["https://cdn.example.test/tpr/wow/config/"+buildKey[:2]+"/"+buildKey[2:4]+"/"+buildKey] = definitionResponse{status: 200, body: config}
		online, err := f.resolve(ctx, request)
		if err != nil || online.Pin.Data == nil || online.Pin.Data.Product != profile.Product || online.Pin.Data.BuildConfig != buildKey {
			t.Fatalf("%s online: %+v %v", profile.Product, online, err)
		}
		pins[profile.Product] = online.Pin.ID
		request.Offline = true
		calls := len(f.transport.callURLs())
		offline, err := f.resolve(ctx, request)
		if err != nil || offline.Pin.ID != online.Pin.ID || len(f.transport.callURLs()) != calls {
			t.Fatalf("%s offline: %+v %v", profile.Product, offline, err)
		}
		cdn := &cdnFiles{ctx: ctx, store: f.store, metadata: f.metadata, client: f.defs.client, offline: true}
		meta, err := cdn.prepare(*online.Pin.Data)
		if err != nil || meta.Installed.Product != profile.ProductCode || meta.Installed.BuildConfig != buildKey || len(f.transport.callURLs()) != calls {
			t.Fatalf("%s CDN route: %+v %v", profile.Product, meta, err)
		}
	}
	if pins["forever-asia-beta"] == pins["forever-cn-beta"] {
		t.Fatal("same-build beta pins collapsed")
	}
}
