package records

import (
	"bytes"
	"encoding/json"
	"errors"

	"github.com/follenfang/lycheedev/internal/vault"
)

// Route changes affect only where immutable keys are fetched. They never
// resolve versions or replace the caller's BuildConfig/CDNConfig.
func (c *cdnFiles) distribution(product, region string) ([]byte, error) {
	c.routeKey = "cdn-route/" + product + "/" + region
	c.region = region
	doc, err := c.metadata.ReadDocument(c.ctx, c.routeKey)
	if err == nil {
		var ref vault.BlobRef
		if json.Unmarshal(doc.Value, &ref) != nil {
			return nil, ErrRemoteIdentity
		}
		return c.store.ReadBlob(c.ctx, ref, 1<<20)
	}
	if !errors.Is(err, vault.ErrMissingRecord) {
		return nil, err
	}
	doc, err = c.metadata.ReadDocument(c.ctx, "remote-catalog/"+product+"/"+region)
	if err == nil {
		var observed remoteObservation
		if json.Unmarshal(doc.Value, &observed) != nil || observed.ProductCode != product || observed.Region != region {
			return nil, ErrRemoteIdentity
		}
		return c.store.ReadBlob(c.ctx, observed.Distribution, 1<<20)
	}
	if !errors.Is(err, vault.ErrMissingRecord) {
		return nil, err
	}
	if c.offline {
		return nil, ErrRemoteUnavailable
	}
	return c.fetchDistribution()
}

func (c *cdnFiles) fetchDistribution() ([]byte, error) {
	raw, err := fetchRemoteMetadata(c.ctx, c.client, c.catalogURL, 1<<20)
	if err != nil {
		return nil, err
	}
	if _, err = parseDistributionCatalog(raw, c.region); err != nil {
		return nil, err
	}
	doc, err := c.metadata.ReadDocument(c.ctx, c.routeKey)
	if err != nil && !errors.Is(err, vault.ErrMissingRecord) {
		return nil, err
	}
	ref, err := c.store.PublishBlob(c.ctx, vault.BlobInput{Reader: bytes.NewReader(raw), MaxBytes: 1 << 20})
	if err != nil {
		return nil, err
	}
	value, _ := json.Marshal(ref)
	err = c.metadata.CommitDocuments(c.ctx, vault.Mutation{Key: c.routeKey, Value: value, ExpectedGeneration: doc.Generation})
	if errors.Is(err, vault.ErrGeneration) {
		err = nil
	} // A concurrent refresh may win; this call retains its own route.
	return raw, err
}

func (c *cdnFiles) refreshRoute() (bool, error) {
	if c.offline || c.routeRefreshed || c.catalogURL == "" {
		return false, nil
	}
	if err := c.ctx.Err(); err != nil {
		return false, err
	}
	c.routeRefreshed = true
	raw, err := c.fetchDistribution()
	if err != nil {
		return false, err
	}
	c.route, err = parseDistributionCatalog(raw, c.region)
	return err == nil, err
}
