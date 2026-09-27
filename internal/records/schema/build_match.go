package schema

import "strings"

// MatchesBuild applies the same exact four-component build ordering as DBD
// selection to a comma-separated list of inclusive ranges or exact builds.
func MatchesBuild(build, constraints string) (bool, error) {
	v, err := parseVersion(build)
	if err != nil {
		return false, err
	}
	matched := false
	for _, part := range strings.Split(constraints, ",") {
		ends := strings.Split(strings.TrimSpace(part), "-")
		if len(ends) < 1 || len(ends) > 2 {
			return false, ErrFormat
		}
		low, err := parseVersion(strings.TrimSpace(ends[0]))
		if err != nil {
			return false, err
		}
		high := low
		if len(ends) == 2 {
			high, err = parseVersion(strings.TrimSpace(ends[1]))
			if err != nil {
				return false, err
			}
		}
		if compare(low, high) > 0 {
			return false, ErrFormat
		}
		matched = matched || compare(v, low) >= 0 && compare(v, high) <= 0
	}
	return matched, nil
}
