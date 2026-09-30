package memory

import (
	"context"
	"errors"
	"sync"
)

// ErrBudget leaves unobserved memory unknown. A Session never renews its budget.
var ErrBudget = errors.New("memory.operation_budget")

var ErrSessionOwnership = errors.New("memory.session_ownership")

type Budget struct {
	MaxReadCalls          uint64
	MaxRequestedBytes     uint64
	MaxCandidates         uint64
	MaxLearningCandidates uint64
	MaxLearningBytes      uint64
}

// Stats separates physical copies from the logical coverage plan. It contains
// counts only, never process bytes or record payloads.
type Stats struct {
	LearningBytes      uint64 `json:"learningBytes"`
	HeaderRejected     uint64 `json:"headerRejected"`
	IntegrityRejected  uint64 `json:"integrityRejected"`
	PredicateRejected  uint64 `json:"predicateRejected"`
	LocalStopped       bool   `json:"localStopped"`
	DeadlineExceeded   bool   `json:"deadlineExceeded"`
	MappingQueries     uint64 `json:"mappingQueries"`
	RPMCalls           uint64 `json:"rpmCalls"`
	ReadCalls          uint64 `json:"readCalls"`
	RequestedBytes     uint64 `json:"requestedBytes"`
	ActualBytes        uint64 `json:"actualBytes"`
	ShortReads         uint64 `json:"shortReads"`
	VerifyCalls        uint64 `json:"verifyCalls"`
	RegionCalls        uint64 `json:"regionCalls"`
	Candidates         uint64 `json:"candidates"`
	LearningCandidates uint64 `json:"learningCandidates"`
	Learned            uint64 `json:"learned"`
	Validated          uint64 `json:"validated"`
	Rejected           uint64 `json:"rejected"`
	Cancelled          bool   `json:"cancelled"`
	BudgetExhausted    bool   `json:"budgetExhausted"`
	LearningExhausted  bool   `json:"learningExhausted"`
}

// Session is owned by one operation and may be shared by concurrent lookups.
// Source still verifies identity and checks the current mapping on every read.
type Session struct {
	mu               sync.Mutex
	budget           Budget
	stats            Stats
	learningReserved uint64
}

func NewSession(b Budget) *Session {
	if b.MaxReadCalls == 0 {
		b.MaxReadCalls = 1 << 20
	}
	if b.MaxRequestedBytes == 0 {
		b.MaxRequestedBytes = 256 << 30
	}
	if b.MaxCandidates == 0 {
		b.MaxCandidates = 1 << 20
	}
	if b.MaxLearningCandidates == 0 {
		b.MaxLearningCandidates = 4096
	}
	if b.MaxLearningBytes == 0 {
		b.MaxLearningBytes = 64 << 20
	}
	return &Session{budget: b}
}
func (s *Session) Stats() Stats { s.mu.Lock(); defer s.mu.Unlock(); return s.stats }
func (s *Session) Err() error {
	if s.Stats().BudgetExhausted {
		return ErrBudget
	}
	return nil
}
func (s *Session) update(f func(*Stats)) { s.mu.Lock(); defer s.mu.Unlock(); f(&s.stats) }

type localScopeKey struct{}

func localContext(ctx, parent context.Context) context.Context {
	return context.WithValue(ctx, localScopeKey{}, parent)
}
func (s *Session) cancelled(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		s.update(func(st *Stats) {
			if parent, ok := ctx.Value(localScopeKey{}).(context.Context); ok && parent.Err() == nil {
				st.LocalStopped = true
			} else if errors.Is(err, context.DeadlineExceeded) {
				st.DeadlineExceeded = true
			} else {
				st.Cancelled = true
			}
		})
		return err
	}
	return nil
}
func (s *Session) learningIndex(view, pattern []byte, index func([]byte, []byte) int) (int, bool) {
	s.mu.Lock()
	n := uint64(len(view))
	remaining := s.budget.MaxLearningBytes - s.stats.LearningBytes - s.learningReserved
	if s.stats.LearningExhausted || remaining < uint64(len(pattern)) {
		s.stats.LearningExhausted = true
		s.mu.Unlock()
		return -1, false
	}
	if n > remaining {
		n = remaining
	}
	s.learningReserved += n
	s.mu.Unlock()
	at := index(view[:n], pattern)
	used := n
	if at >= 0 {
		used = uint64(at + len(pattern))
	}
	s.mu.Lock()
	s.learningReserved -= n
	s.stats.LearningBytes += used
	s.mu.Unlock()
	return at, true
}
func (s *Session) candidate(learning bool) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if learning {
		if s.stats.LearningCandidates >= s.budget.MaxLearningCandidates {
			s.stats.LearningExhausted = true
			return false
		}
		s.stats.LearningCandidates++
		return true
	}
	if s.stats.BudgetExhausted || s.stats.Candidates >= s.budget.MaxCandidates {
		s.stats.BudgetExhausted = true
		return false
	}
	s.stats.Candidates++
	return true
}

type sessionSource struct {
	Source
	session *Session
}
type sessionOwner interface{ memorySession() *Session }

func (s *sessionSource) memorySession() *Session { return s.session }
func sourceSession(src Source) *Session {
	if owner, ok := src.(sessionOwner); ok {
		return owner.memorySession()
	}
	return nil
}
func (s *Session) Source(src Source) Source {
	if existing := sourceSession(src); existing != nil {
		if existing == s {
			return src
		}
		return &sessionRejectedSource{session: s}
	}
	return &sessionSource{Source: src, session: s}
}
func measured(src Source, session *Session) (Source, *Session) {
	if existing := sourceSession(src); existing != nil {
		if session == nil || session == existing {
			return src, existing
		}
		return session.Source(src), session
	}
	if session == nil {
		session = NewSession(Budget{})
	}
	return session.Source(src), session
}

// Conflicting owners cannot silently bypass the caller's explicit budget.
type sessionRejectedSource struct{ session *Session }

func (s *sessionRejectedSource) memorySession() *Session { return s.session }
func (*sessionRejectedSource) Verify(ctx context.Context) error {
	return errors.Join(ctx.Err(), ErrSessionOwnership)
}
func (*sessionRejectedSource) Regions(ctx context.Context) ([]Region, error) {
	return nil, errors.Join(ctx.Err(), ErrSessionOwnership)
}
func (*sessionRejectedSource) Read(ctx context.Context, _ uint64, _ []byte) (int, error) {
	return 0, errors.Join(ctx.Err(), ErrSessionOwnership)
}
func (s *sessionSource) Verify(ctx context.Context) error {
	if err := s.session.cancelled(ctx); err != nil {
		return err
	}
	s.session.update(func(st *Stats) { st.VerifyCalls++ })
	return s.Source.Verify(ctx)
}
func (s *sessionSource) Regions(ctx context.Context) ([]Region, error) {
	if err := s.session.cancelled(ctx); err != nil {
		return nil, err
	}
	s.session.update(func(st *Stats) { st.RegionCalls++ })
	if src, ok := s.Source.(interface {
		regionsMeasured(context.Context, *Session) ([]Region, error)
	}); ok {
		return src.regionsMeasured(ctx, s.session)
	}
	return s.Source.Regions(ctx)
}
func (s *sessionSource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if err := s.session.cancelled(ctx); err != nil {
		return 0, err
	}
	s.session.mu.Lock()
	st, budget := &s.session.stats, s.session.budget
	if st.BudgetExhausted || st.ReadCalls >= budget.MaxReadCalls || uint64(len(b)) > budget.MaxRequestedBytes-st.RequestedBytes {
		st.BudgetExhausted = true
		s.session.mu.Unlock()
		return 0, ErrBudget
	}
	st.ReadCalls++
	st.RequestedBytes += uint64(len(b))
	s.session.mu.Unlock()
	var n int
	var err error
	if src, ok := s.Source.(interface {
		readMeasured(context.Context, uint64, []byte, *Session) (int, error)
	}); ok {
		n, err = src.readMeasured(ctx, address, b, s.session)
	} else {
		n, err = s.Source.Read(ctx, address, b)
	}
	_ = s.session.cancelled(ctx)
	s.session.update(func(st *Stats) {
		if n >= 0 && n <= len(b) {
			st.ActualBytes += uint64(n)
		}
		if n != len(b) {
			st.ShortReads++
		}
	})
	return n, err
}
func statsDelta(after, before Stats) Stats {
	after.LearningBytes -= before.LearningBytes
	after.HeaderRejected -= before.HeaderRejected
	after.IntegrityRejected -= before.IntegrityRejected
	after.PredicateRejected -= before.PredicateRejected
	after.MappingQueries -= before.MappingQueries
	after.RPMCalls -= before.RPMCalls
	after.ReadCalls -= before.ReadCalls
	after.RequestedBytes -= before.RequestedBytes
	after.ActualBytes -= before.ActualBytes
	after.ShortReads -= before.ShortReads
	after.VerifyCalls -= before.VerifyCalls
	after.RegionCalls -= before.RegionCalls
	after.Candidates -= before.Candidates
	after.LearningCandidates -= before.LearningCandidates
	after.Learned -= before.Learned
	after.Validated -= before.Validated
	after.Rejected -= before.Rejected
	return after
}

// Delta reports counter changes; exhaustion/cancellation flags remain sticky.
func (s Stats) Delta(before Stats) Stats { return statsDelta(s, before) }

// Add aggregates disjoint counter intervals; diagnostic flags are sticky.
func (s Stats) Add(other Stats) Stats {
	s.ReadCalls += other.ReadCalls
	s.RequestedBytes += other.RequestedBytes
	s.ActualBytes += other.ActualBytes
	s.ShortReads += other.ShortReads
	s.VerifyCalls += other.VerifyCalls
	s.RegionCalls += other.RegionCalls
	s.MappingQueries += other.MappingQueries
	s.RPMCalls += other.RPMCalls
	s.Candidates += other.Candidates
	s.LearningCandidates += other.LearningCandidates
	s.LearningBytes += other.LearningBytes
	s.Learned += other.Learned
	s.Validated += other.Validated
	s.Rejected += other.Rejected
	s.HeaderRejected += other.HeaderRejected
	s.IntegrityRejected += other.IntegrityRejected
	s.PredicateRejected += other.PredicateRejected
	s.Cancelled = s.Cancelled || other.Cancelled
	s.DeadlineExceeded = s.DeadlineExceeded || other.DeadlineExceeded
	s.LocalStopped = s.LocalStopped || other.LocalStopped
	s.BudgetExhausted = s.BudgetExhausted || other.BudgetExhausted
	s.LearningExhausted = s.LearningExhausted || other.LearningExhausted
	return s
}
