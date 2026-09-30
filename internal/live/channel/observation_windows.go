//go:build windows && amd64

package channel

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// This is invocation accounting. Resume opens a new Native and a new physical
// budget; its authoritative durable goal deadline is never extended here.
type ObservationMetrics struct {
	Scope           string                    `json:"scope"`
	PID             uint32                    `json:"pid"`
	ProcessCreated  uint64                    `json:"processCreated,string"`
	Runtime         string                    `json:"runtime,omitempty"`
	DeadlineMS      int64                     `json:"deadlineMS,omitempty"`
	Lookups         int                       `json:"lookups"`
	StageTruncated  bool                      `json:"stageTruncated"`
	Total           memory.Stats              `json:"total"`
	Stages          []ObservationStage        `json:"stages"`
	InputPredicates InputPredicateDiagnostics `json:"inputPredicates"`
	InputTimings    InputTimingDiagnostics    `json:"inputTimings"`
	InputRefresh    InputRefreshDiagnostics   `json:"inputRefresh"`
	InputOptical    InputOpticalDiagnostics   `json:"inputOptical"`
}

type InputTimingSample struct {
	AcceptedAtOffsetMillis  *int64 `json:"acceptedAtOffsetMillis"`
	DrainMillis             *int64 `json:"drainMillis"`
	AcceptedSampleAgeMillis *int64 `json:"acceptedSampleAgeMillis"`
	FinishSampleAgeMillis   *int64 `json:"finishSampleAgeMillis"`
	Outcome                 string `json:"outcome"`
}

type InputTimingDiagnostics struct {
	Samples   []InputTimingSample `json:"samples"`
	Truncated bool                `json:"truncated"`
}

type InputRefreshDiagnostics struct {
	Attempts          uint64 `json:"attempts"`
	ImmediateHit      uint64 `json:"immediateHit"`
	EdgeWaitSucceeded uint64 `json:"edgeWaitSucceeded"`
	EdgeWaitExpired   uint64 `json:"edgeWaitExpired"`
	EdgeWaitOther     uint64 `json:"edgeWaitOther"`
	AfterEdgeHit      uint64 `json:"afterEdgeHit"`
}

const inputTimingLimit = 16

type inputFreshStamp struct{ sample, age int64 }

// Counts cover this invocation. Ages summarize its latest input observation,
// using the same uptime clock as the validator; nil means no qualified sample.
// No payload, actor, address or absolute sample time is retained here.
type InputPredicateDiagnostics struct {
	Observations           uint64 `json:"observations"`
	PredicateCalls         uint64 `json:"predicateCalls"`
	Fresh                  uint64 `json:"fresh"`
	Stale                  uint64 `json:"stale"`
	StaleAge               uint64 `json:"staleAge"`
	StaleAfterInput        uint64 `json:"staleAfterInput"`
	TargetChanged          uint64 `json:"targetChanged"`
	RuntimeMismatch        uint64 `json:"runtimeMismatch"`
	OwnerMismatch          uint64 `json:"ownerMismatch"`
	FenceMismatch          uint64 `json:"fenceMismatch"`
	SlotMismatch           uint64 `json:"slotMismatch"`
	ActorMismatch          uint64 `json:"actorMismatch"`
	BuildMismatch          uint64 `json:"buildMismatch"`
	ClockInvalid           uint64 `json:"clockInvalid"`
	StructInvalid          uint64 `json:"structInvalid"`
	FinishFresh            uint64 `json:"finishFresh"`
	FinishStale            uint64 `json:"finishStale"`
	NewestSampleAgeMillis  *int64 `json:"newestSampleAgeMillis"`
	ScanEndSampleAgeMillis *int64 `json:"scanEndSampleAgeMillis"`
}

type inputPredicateCollector struct {
	mu          sync.Mutex
	diagnostics InputPredicateDiagnostics
	newest      int64
	haveSample  bool
	freshStamps [8]inputFreshStamp
	freshCount  int
	freshNext   int
	timing      InputTimingSample
	haveTiming  bool
	refresh     InputRefreshDiagnostics
}

func (c *inputPredicateCollector) candidate(s InputObservation, err error, e bridge.SlotEnvelope, after, now int64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	d := &c.diagnostics
	d.PredicateCalls++
	switch {
	case err == nil:
		d.Fresh++
		found := false
		for i := 0; i < c.freshCount; i++ {
			if c.freshStamps[i].sample == s.SampleMillis {
				c.freshStamps[i].age = now - s.SampleMillis
				found = true
				break
			}
		}
		if !found {
			c.freshStamps[c.freshNext] = inputFreshStamp{s.SampleMillis, now - s.SampleMillis}
			c.freshNext = (c.freshNext + 1) % len(c.freshStamps)
			if c.freshCount < len(c.freshStamps) {
				c.freshCount++
			}
		}
	case errors.Is(err, errInputStateStale):
		d.Stale++
		if s.SampleMillis < now-500 {
			d.StaleAge++
		}
		if s.SampleMillis < after+100 {
			d.StaleAfterInput++
		}
	case err.Error() == "live.channel_input_target_changed":
		d.TargetChanged++
		if s.Runtime != e.Runtime {
			d.RuntimeMismatch++
		}
		if s.Owner != e.Owner && !(e.Action == "bind" && s.Owner == "") {
			d.OwnerMismatch++
		}
		if s.Owner != "" && s.Fence != e.Fence {
			d.FenceMismatch++
		}
		if e.Action != "reload" && s.NextSlot != slotStart(e) {
			d.SlotMismatch++
		}
		if s.GUID != e.GUID {
			d.ActorMismatch++
		}
		if s.Build != e.Build {
			d.BuildMismatch++
		}
		return
	case err.Error() == "live.channel_input_clock_invalid":
		d.ClockInvalid++
		return
	default:
		d.StructInvalid++
		return
	}
	// Both success and stale reach this point only after format, identity and
	// clock validation. Target/clock/structure failures never contribute ages.
	if !c.haveSample || s.SampleMillis > c.newest {
		c.newest, c.haveSample = s.SampleMillis, true
		age := now - s.SampleMillis
		d.NewestSampleAgeMillis = &age
	}
}

// Only positive lookup observations occupy the bounded timing samples. The
// last eight qualified fresh stamps exist only inside this invocation call;
// serialized evidence contains relative ages, never absolute sample clocks.
func (c *inputPredicateCollector) positive(records []memory.Record, timing *memory.InputLookupTiming, now int64, err error, lookupError bool) {
	if len(records) == 0 && timing == nil {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	sample := InputTimingSample{Outcome: "rejected"}
	if lookupError {
		sample.Outcome = "lookup_error"
	} else if err == nil {
		sample.Outcome = "fresh"
	} else if errors.Is(err, ErrInputObservationStale) {
		sample.Outcome = "stale"
	}
	if timing != nil {
		offset, drain := timing.AcceptedAtOffsetMillis, timing.DrainMillis
		sample.AcceptedAtOffsetMillis, sample.DrainMillis = &offset, &drain
	}
	var state InputObservation
	if len(records) > 0 && json.Unmarshal(records[0].Payload, &state) == nil {
		for i := 0; i < c.freshCount; i++ {
			if c.freshStamps[i].sample == state.SampleMillis {
				age, finish := c.freshStamps[i].age, now-state.SampleMillis
				sample.AcceptedSampleAgeMillis, sample.FinishSampleAgeMillis = &age, &finish
				break
			}
		}
	}
	c.timing, c.haveTiming = sample, true
}

func (c *inputPredicateCollector) refreshAttempt() { c.mu.Lock(); c.refresh.Attempts++; c.mu.Unlock() }
func (c *inputPredicateCollector) refreshHit(afterEdge bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if afterEdge {
		c.refresh.AfterEdgeHit++
	} else {
		c.refresh.ImmediateHit++
	}
}
func (c *inputPredicateCollector) edgeWait(err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err == nil {
		c.refresh.EdgeWaitSucceeded++
	} else if errors.Is(err, context.DeadlineExceeded) {
		c.refresh.EdgeWaitExpired++
	} else {
		c.refresh.EdgeWaitOther++
	}
}

func (c *inputPredicateCollector) scanEnd(now int64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.haveSample {
		age := now - c.newest
		c.diagnostics.ScanEndSampleAgeMillis = &age
	}
}

func (c *inputPredicateCollector) finish(err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err == nil {
		c.diagnostics.FinishFresh++
	}
	if errors.Is(err, ErrInputObservationStale) {
		c.diagnostics.FinishStale++
	}
}

func (n *Native) recordInputPredicates(c *inputPredicateCollector) {
	c.mu.Lock()
	delta := c.diagnostics
	timing, haveTiming, refresh := c.timing, c.haveTiming, c.refresh
	c.mu.Unlock()
	o := n.observation
	o.inputMu.Lock()
	defer o.inputMu.Unlock()
	d := &o.inputPredicates
	d.Observations++
	d.PredicateCalls += delta.PredicateCalls
	d.Fresh += delta.Fresh
	d.Stale += delta.Stale
	d.StaleAge += delta.StaleAge
	d.StaleAfterInput += delta.StaleAfterInput
	d.TargetChanged += delta.TargetChanged
	d.RuntimeMismatch += delta.RuntimeMismatch
	d.OwnerMismatch += delta.OwnerMismatch
	d.FenceMismatch += delta.FenceMismatch
	d.SlotMismatch += delta.SlotMismatch
	d.ActorMismatch += delta.ActorMismatch
	d.BuildMismatch += delta.BuildMismatch
	d.ClockInvalid += delta.ClockInvalid
	d.StructInvalid += delta.StructInvalid
	d.FinishFresh += delta.FinishFresh
	d.FinishStale += delta.FinishStale
	if haveTiming {
		if len(o.inputTimings.Samples) < inputTimingLimit {
			o.inputTimings.Samples = append(o.inputTimings.Samples, timing)
		} else {
			o.inputTimings.Truncated = true
		}
	}
	o.inputRefresh.Attempts += refresh.Attempts
	o.inputRefresh.ImmediateHit += refresh.ImmediateHit
	o.inputRefresh.EdgeWaitSucceeded += refresh.EdgeWaitSucceeded
	o.inputRefresh.EdgeWaitExpired += refresh.EdgeWaitExpired
	o.inputRefresh.EdgeWaitOther += refresh.EdgeWaitOther
	o.inputRefresh.AfterEdgeHit += refresh.AfterEdgeHit
	// A deferred cadence tick performs no predicate work and must not erase the
	// last actual lookup's bounded age summary.
	if delta.PredicateCalls > 0 {
		d.NewestSampleAgeMillis = delta.NewestSampleAgeMillis
		d.ScanEndSampleAgeMillis = delta.ScanEndSampleAgeMillis
	}
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
	inputMu         sync.Mutex
	inputPredicates InputPredicateDiagnostics
	inputTimings    InputTimingDiagnostics
	inputRefresh    InputRefreshDiagnostics
	session         *memory.Session
	pid             uint32
	created         uint64
	image           string
	runtime         string
	deadline        time.Time
	lookups         int
	stages          []ObservationStage
	stageTruncated  bool
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
	o.inputMu.Lock()
	metrics.InputPredicates = o.inputPredicates
	metrics.InputTimings = InputTimingDiagnostics{Samples: append([]InputTimingSample{}, o.inputTimings.Samples...), Truncated: o.inputTimings.Truncated}
	metrics.InputRefresh = o.inputRefresh
	o.inputMu.Unlock()
	metrics.InputOptical = n.inputOptical.Snapshot()
	if !o.deadline.IsZero() {
		metrics.DeadlineMS = o.deadline.UnixMilli()
	}
	return metrics
}
