package error_returns

// AppError is a concrete error type with a value receiver Error().
type AppError struct{ Code int }

func (e AppError) Error() string { return "app error" }

// PtrError is a concrete error type with a pointer receiver Error().
type PtrError struct{ Detail string }

func (e *PtrError) Error() string { return "ptr error" }

// Widget is a normal type (does not implement error).
type Widget struct{ ID int }

// Single *T returns where T implements error → should become "error"
func NewAppError() *AppError    { return &AppError{Code: 1} }
func NewPtrError() *PtrError    { return &PtrError{Detail: "x"} }
func WrapAppError() *AppError   { return &AppError{Code: 2} }

// Normal pointer return (not error impl) → should remain Ref<Widget>
func NewWidget() *Widget { return &Widget{ID: 1} }

// Nilable pointer return (not error impl) → should remain Option<Ref<Widget>>
func GetWidget() *Widget { return nil }

// Multi-return with error last → Result<Ref<AppError>, error>, not error
func ParseAppError() (*AppError, error) { return &AppError{}, nil }

// Single error interface return (not *T) → Result<(), error>
func Validate() error { return nil }
