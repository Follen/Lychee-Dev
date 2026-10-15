//go:build !windows

package resource

func newQueryBudget(_ int) (*Budget, error) { return New(DefaultLimits()), nil }
