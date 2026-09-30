//go:build windows && amd64

package channel

import (
	"bytes"
	"context"
	"errors"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// This is invocation accounting. Resume opens a new Native and a new physical
// budget; its authoritative durable goal deadline is never extended here.
type ObservationMetrics struct {
	Scope          string             `json:"scope"`
	PID            uint32             `json:"pid"`
	ProcessCreated uint64             `json:"processCreated,string"`
	Runtime        string             `json:"runtime,omitempty"`
	DeadlineMS     int64              `json:"deadlineMS,omitempty"`
	Lookups        int                `json:"lookups"`
	StageTruncated bool               `json:"stageTruncated"`
	Total          memory.Stats       `json:"total"`
	Stages         []ObservationStage `json:"stages"`
}

type ObservationStage struct {
	Kind          bridge.MemoryKind `json:"kind"`
	Path          string            `json:"path"`
	Timed         bool              `json:"timed"`
	Calls         int               `json:"calls"`
	ElapsedMillis int64             `json:"elapsedMillis"`
	Stats         memory.Stats      `json:"stats"`
}

type nativeObservation struct {
	session        *memory.Session
	pid            uint32
	created        uint64
	image          string
	runtime        string
	deadline       time.Time
	lookups        int
	stages         []ObservationStage
	stageTruncated bool
}

const observationStageLimit = 32

func (n *Native) memorySession() *memory.Session {
	if n.observation == nil {
		n.observation = &nativeObservation{session: memory.NewSession(memory.Budget{}), pid: n.Target.ProcessID, created: n.Target.ProcessStartedAt, image: n.Target.Executable}
	}
	return n.observation.session
}

func (n *Native) memorySource(src memory.Source) memory.Source { return n.memorySession().Source(src) }

func (n *Native) beginObservation(ctx context.Context, identity Identity) error {
	n.memorySession()
	o := n.observation
	if o.pid != n.Target.ProcessID || o.created != n.Target.ProcessStartedAt || !strings.EqualFold(o.image, n.Target.Executable) {
		return errors.New("live.channel_observation_target_changed")
	}
	if n.Process != nil && (n.Process.PID != o.pid || n.Process.Created != o.created || !strings.EqualFold(n.Process.Image, o.image)) {
		return errors.New("live.channel_observation_target_changed")
	}
	deadline, bounded := ctx.Deadline()
	if !bounded {
		return errors.New("live.channel_deadline_required")
	}
	if o.deadline.IsZero() || deadline.Before(o.deadline) {
		o.deadline = deadline
	}
	if o.runtime != identity.Runtime {
		n.observationRuntime(identity.Runtime)
	}
	return ctx.Err()
}

func (n *Native) observationRuntime(runtime string) {
	n.memorySession()
	if n.observation.runtime == runtime {
		return
	}
	// New runtime must establish its own input eligibility, without renewing
	// I/O credit. General hints remain untrusted, scoped by each exact selector.
	n.inputWait.reset()
	n.inputHintRuntime, n.inputHintMillis = "", 0
	if n.inputSignal != nil && n.inputSignal.runtime != runtime {
		n.inputSignal.close()
		n.inputSignal = nil
	}
	n.observation.runtime = runtime
}

func (n *Native) observationContext(ctx context.Context) (context.Context, context.CancelFunc) {
	n.memorySession()
	o := n.observation
	// Only initialize here: a nested nearby/replacement deadline bounds that
	// stage and must not expire the outer operation after a successful stage.
	if deadline, ok := ctx.Deadline(); ok && o.deadline.IsZero() {
		o.deadline = deadline
	}
	if !o.deadline.IsZero() {
		return context.WithDeadline(ctx, o.deadline)
	}
	return ctx, func() {}
}

func localLookupMiss(ctx context.Context, err error) bool {
	return ctx.Err() == nil && (errors.Is(err, memory.ErrNearbyBudget) || errors.Is(err, context.DeadlineExceeded)) && !errors.Is(err, memory.ErrBudget)
}

func (n *Native) findAuthorizedBody(ctx context.Context, head memory.Record, headSelector, bodySelector memory.Selector) (memory.LookupResult, error) {
	return n.findAuthorizedBodyFrom(ctx, n.Process, head, headSelector, bodySelector)
}

func (n *Native) findAuthorizedBodyFrom(ctx context.Context, source memory.Source, head memory.Record, headSelector, bodySelector memory.Selector) (memory.LookupResult, error) {
	ctx, cancel := n.observationContext(ctx)
	defer cancel()
	var body memory.LookupResult
	var err error
	// A HEAD position chooses a neighborhood, never an adjacent BODY offset.
	// With cache disabled every query still uses complete process discovery.
	if n.Hints != nil {
		if n.observation.lookups >= observationLimit {
			return body, errors.New("live.channel_observation_budget")
		}
		body, err = memory.FindNearbySeeded(ctx, source, bodySelector, []memory.Hint{{Address: head.Address, Header: head.Header}}, n.memorySession())
		n.recordLookup(bodySelector, body)
		if err != nil && !localLookupMiss(ctx, err) {
			return body, err
		}
	}
	if len(body.Records) == 0 {
		body, err = n.findPath(ctx, source, bodySelector, true, false)
		if err != nil {
			return body, err
		}
	}
	if len(body.Records) == 0 {
		return body, nil
	}
	// Final acceptance uses a fresh HEAD and process identity after the BODY.
	// A changed/missing HEAD cannot authorize the retained payload we found.
	measured := n.memorySource(source)
	fresh, err := memory.ReadRecord(ctx, measured, head.Address, headSelector)
	if err != nil || fresh.Header != head.Header || !bytes.Equal(fresh.Payload, head.Payload) {
		body.Records = nil
		if ctx.Err() != nil {
			return body, ctx.Err()
		}
		if budgetErr := n.memorySession().Err(); budgetErr != nil {
			return body, budgetErr
		}
		return body, ErrPending
	}
	if err := measured.Verify(ctx); err != nil {
		body.Records = nil
		return body, err
	}
	return body, nil
}

func addStats(a, b memory.Stats) memory.Stats {
	return a.Add(b)
}

func (n *Native) recordObservationStage(s memory.Selector, found memory.LookupResult) {
	n.memorySession()
	o := n.observation
	o.lookups++
	for i := range o.stages {
		stage := &o.stages[i]
		if stage.Kind == s.Kind && stage.Path == found.Path {
			stage.Calls++
			stage.ElapsedMillis += found.ElapsedMillis
			stage.Stats = addStats(stage.Stats, found.Stats)
			return
		}
	}
	if len(o.stages) < observationStageLimit-1 {
		o.stages = append(o.stages, ObservationStage{Kind: s.Kind, Path: found.Path, Timed: true, Calls: 1, ElapsedMillis: found.ElapsedMillis, Stats: found.Stats})
	} else {
		o.stageTruncated = true
	}
}

func (n *Native) observationMetrics() *ObservationMetrics {
	if n.observation == nil {
		return nil
	}
	o := n.observation
	stages := append([]ObservationStage{}, o.stages...)
	var lookupStats memory.Stats
	for _, stage := range stages {
		lookupStats = addStats(lookupStats, stage.Stats)
	}
	total := o.session.Stats()
	// Point input verification and runtime-replacement snapshots are included
	// in the total, even though they are not a Find. Keep disjoint accounting.
	residual := "direct_validation"
	if o.stageTruncated {
		residual = "unattributed"
	}
	stages = append(stages, ObservationStage{Path: residual, Stats: total.Delta(lookupStats)})
	metrics := &ObservationMetrics{Scope: "invocation", PID: o.pid, ProcessCreated: o.created, Runtime: o.runtime, Lookups: o.lookups, StageTruncated: o.stageTruncated, Total: total, Stages: stages}
	if !o.deadline.IsZero() {
		metrics.DeadlineMS = o.deadline.UnixMilli()
	}
	return metrics
}
