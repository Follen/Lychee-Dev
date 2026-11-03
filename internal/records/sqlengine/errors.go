package sqlengine

import (
	"context"
	"errors"
	"fmt"
	"strings"

	duckdb "github.com/duckdb/duckdb-go/v2"
)

var (
	ErrSyntax       = errors.New("sql.invalid_syntax")
	ErrReadOnly     = errors.New("sql.read_only_required")
	ErrBinding      = errors.New("sql.unresolved_binding")
	ErrUnsupported  = errors.New("sql.unsupported_type")
	ErrBudget       = errors.New("sql.budget_exceeded")
	ErrType         = errors.New("sql.type_mismatch")
	ErrNumericRange = errors.New("sql.numeric_range")
	ErrCardinality  = errors.New("sql.invalid_cardinality")
)

func engineError(ctx context.Context, err error) error {
	if err == nil {
		return nil
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	var native *duckdb.Error
	if !errors.As(err, &native) {
		return err
	}
	var cause error
	switch native.Type {
	case duckdb.ErrorTypeParser, duckdb.ErrorTypeSyntax:
		cause = ErrSyntax
	case duckdb.ErrorTypeBinder, duckdb.ErrorTypeCatalog, duckdb.ErrorTypeParameterNotResolved, duckdb.ErrorTypeParameterNotAllowed:
		cause = ErrBinding
	case duckdb.ErrorTypeOutOfMemory:
		cause = ErrBudget
	case duckdb.ErrorTypeConversion, duckdb.ErrorTypeMismatchType, duckdb.ErrorTypeInvalidType:
		cause = ErrType
	case duckdb.ErrorTypeOutOfRange, duckdb.ErrorTypeDecimal, duckdb.ErrorTypeDivideByZero:
		cause = ErrNumericRange
	case duckdb.ErrorTypePermission, duckdb.ErrorTypeNotImplemented, duckdb.ErrorTypeMissingExtension, duckdb.ErrorTypeAutoLoad:
		cause = ErrUnsupported
	case duckdb.ErrorTypeInvalidInput:
		cause = ErrType
		if strings.Contains(native.Error(), "More than one row returned by a subquery") {
			cause = ErrCardinality
		}
	default:
		return err
	}
	return fmt.Errorf("%w: %w", cause, err)
}
