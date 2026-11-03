package sqlengine

import (
	"encoding/json"
	"fmt"
	"math"
	"math/big"
	"strconv"
	"strings"
	"time"

	duckdb "github.com/duckdb/duckdb-go/v2"
)

func parameterValue(value any) (any, error) {
	switch v := value.(type) {
	case nil, bool, string, int64, uint64:
		return v, nil
	case int:
		return int64(v), nil
	case int32:
		return int64(v), nil
	case uint32:
		return uint64(v), nil
	case float64:
		if math.IsNaN(v) || math.IsInf(v, 0) {
			return nil, ErrNumericRange
		}
		return v, nil
	case json.Number:
		if !strings.ContainsAny(string(v), ".eE") {
			if n, err := strconv.ParseInt(string(v), 10, 64); err == nil {
				return n, nil
			}
			if n, err := strconv.ParseUint(string(v), 10, 64); err == nil {
				return n, nil
			}
			return nil, ErrNumericRange
		}
		n, err := strconv.ParseFloat(string(v), 64)
		if err != nil {
			return nil, ErrNumericRange
		}
		return parameterValue(n)
	default:
		return nil, fmt.Errorf("%w: parameter type %T", ErrType, value)
	}
}

func wideInteger(value any) (*big.Int, error) {
	switch v := value.(type) {
	case int64:
		return big.NewInt(v), nil
	case uint64:
		return new(big.Int).SetUint64(v), nil
	case *big.Int:
		return v, nil
	default:
		return nil, fmt.Errorf("%w: integer metadata type %T", ErrType, value)
	}
}

// JSON numbers retain exact integers and decimals. Native numeric types are
// reported with the columns; clients must not round these through binary64.
func outputValue(value any) (any, error) {
	switch v := value.(type) {
	case nil, bool, string, int64, uint64, []byte, time.Time:
		return v, nil
	case int8:
		return int64(v), nil
	case int16:
		return int64(v), nil
	case int32:
		return int64(v), nil
	case uint8:
		return uint64(v), nil
	case uint16:
		return uint64(v), nil
	case uint32:
		return uint64(v), nil
	case float32:
		return outputValue(float64(v))
	case float64:
		if math.IsNaN(v) || math.IsInf(v, 0) {
			return nil, ErrNumericRange
		}
		return v, nil
	case *big.Int:
		if v.IsInt64() {
			return v.Int64(), nil
		}
		if v.IsUint64() {
			return v.Uint64(), nil
		}
		return json.Number(v.String()), nil
	case duckdb.Decimal:
		return json.Number(v.String()), nil
	case duckdb.UUID:
		return v.String(), nil
	case duckdb.Interval:
		return v, nil
	case []any:
		result := make([]any, len(v))
		for i, item := range v {
			var err error
			result[i], err = outputValue(item)
			if err != nil {
				return nil, err
			}
		}
		return result, nil
	case map[string]any:
		result := make(map[string]any, len(v))
		for key, item := range v {
			var err error
			result[key], err = outputValue(item)
			if err != nil {
				return nil, err
			}
		}
		return result, nil
	default:
		return nil, fmt.Errorf("%w: SQL output type %T; cast it to a supported scalar, list or struct", ErrUnsupported, value)
	}
}
