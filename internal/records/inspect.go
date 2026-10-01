package records

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type AssetReading struct {
	Result  FileReading
	Capture evidence.CaptureRef
}

type RecordReading struct {
	Result  TableReading
	Capture evidence.CaptureRef
}

func InspectDataRecord(ctx context.Context, root, snapshot, name string, query FileQuery, id uint32) (RecordReading, error) {
	return inspectData(ctx, root, snapshot, name, query, &id, nil, 0)
}

func InspectDataPage(ctx context.Context, root, snapshot, name string, query FileQuery, after *uint32, limit int) (RecordReading, error) {
	return inspectData(ctx, root, snapshot, name, query, nil, after, limit)
}

func inspectData(ctx context.Context, root, snapshot, name string, query FileQuery, id, after *uint32, limit int) (RecordReading, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (RecordReading, error) {
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return RecordReading{}, err
		}
		if pin.Data == nil {
			return RecordReading{}, errors.New("records.data_pin_required")
		}
		if _, _, err := selection.DataIdentity(*pin.Data); err != nil {
			return RecordReading{}, err
		}
		query = ensureQueryBudget(query)
		admission, err := s.AcquireDataResources(ctx, vault.DataOrdinary)
		if err != nil {
			return RecordReading{}, err
		}
		defer admission.Close()
		query.admitted = true
		bundle, err := OpenDefinitions(s, m).WithBudget(query.budget).Prepare(ctx, pin.Data.DefinitionCommit, name, query.Offline)
		if err != nil {
			return RecordReading{}, err
		}
		query.FileDataID = bundle.Identity.DB2FileDataID
		var reading TableReading
		locator := fmt.Sprintf("db2:%s", bundle.Identity.Name)
		if id != nil {
			reading, err = OpenReader(s).ReadRecord(ctx, *pin.Data, query, bundle, *id)
			locator += fmt.Sprintf(":%d", *id)
		} else {
			reading, err = OpenReader(s).ReadPage(ctx, *pin.Data, query, bundle, after, limit)
		}
		if err != nil {
			return RecordReading{}, err
		}
		raw, err := json.Marshal(reading)
		if err != nil {
			return RecordReading{}, err
		}
		kind, complete, truncated := "data-record", reading.Complete, reading.Truncated
		if reading.Page != nil {
			kind = "data-page"
			complete = reading.Complete
			truncated = reading.Page.More
		}
		capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json", Complete: complete, Truncated: truncated, Provenance: evidence.Provenance{Kind: kind, Locator: locator, Snapshot: snapshot, DataBuild: pin.Data.FullBuild}})
		if err != nil {
			return RecordReading{}, err
		}
		return RecordReading{Result: reading, Capture: capture}, nil
	})
}

func InspectAsset(ctx context.Context, root, snapshot string, query FileQuery) (AssetReading, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (AssetReading, error) {
		reading, _, err := captureAsset(ctx, s, m, snapshot, query)
		return reading, err
	})
}

func captureAsset(ctx context.Context, s *vault.Store, m *vault.Metadata, snapshot string, query FileQuery) (AssetReading, []byte, error) {
	pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
	if err != nil {
		return AssetReading{}, nil, err
	}
	if pin.Data == nil {
		return AssetReading{}, nil, errors.New("records.data_pin_required")
	}
	query = ensureQueryBudget(query)
	admission, err := s.AcquireDataResources(ctx, vault.DataOrdinary)
	if err != nil {
		return AssetReading{}, nil, err
	}
	defer admission.Close()
	query.admitted = true
	reading, err := OpenReader(s).ReadFile(ctx, *pin.Data, query)
	if err != nil {
		return AssetReading{}, nil, err
	}
	if err := query.budget.Charge(resource.Cost{RetainedBytes: reading.Content.Bytes}); err != nil {
		return AssetReading{}, nil, err
	}
	raw, err := s.ReadBlob(ctx, reading.Content, query.ContentBytes)
	if err != nil {
		return AssetReading{}, nil, err
	}
	capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
		Reader: bytes.NewReader(raw), MaxBytes: query.ContentBytes, MediaType: "application/octet-stream", Complete: true,
		Provenance: evidence.Provenance{Kind: "asset", Locator: fmt.Sprintf("casc:fdid:%d:ckey:%s", query.FileDataID, reading.Entry.ContentKey), Snapshot: snapshot, DataBuild: pin.Data.FullBuild},
	})
	if err != nil {
		return AssetReading{}, nil, err
	}
	return AssetReading{Result: reading, Capture: capture}, raw, nil
}
