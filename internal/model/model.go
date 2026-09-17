// Package model defines Rover's versioned records. Execution, evidence and
// acceptance are deliberately separate concepts.
package model

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"regexp"
	"time"
)

const Version = "0.0.1"
const Schema = "rover/v1alpha1"

var validID = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}$`)

func ValidID(s string) bool { return validID.MatchString(s) }

// ID generates a unique identifier with prefix. rand.Read fails only on
// catastrophic syscall exhaustion; callers that need to propagate that
// should call TryID.
func ID(prefix string) string {
	b, err := TryID(prefix)
	if err != nil {
		panic(fmt.Sprintf("system random source unavailable: %v", err))
	}
	return b
}

// TryID is the fallible form of ID.
func TryID(prefix string) (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return prefix + "_" + hex.EncodeToString(b), nil
}

// Digest returns the SHA-256 hex of b. Never fails.
func Digest(b []byte) string { x := sha256.Sum256(b); return hex.EncodeToString(x[:]) }

// Hash marshals v and returns its Digest. Callers needing to treat a
// marshal failure as a recoverable error should use TryHash.
func Hash(v any) string {
	b, err := TryHash(v)
	if err != nil {
		panic(err)
	}
	return b
}

// TryHash is the fallible form of Hash.
func TryHash(v any) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", fmt.Errorf("model: unhashable value: %w", err)
	}
	return Digest(b), nil
}
func Now() string { return time.Now().UTC().Format(time.RFC3339Nano) }

type Config struct {
	Schema string      `json:"schema"`
	Checks []CheckSpec `json:"checks"`
	Policy Policy      `json:"policy"`
}
type Policy struct {
	RequireReview bool     `json:"require_review"`
	ReviewPaths   []string `json:"review_paths,omitempty"`
}
type CheckSpec struct {
	ID          string   `json:"id"`
	Argv        []string `json:"argv"`
	Timeout     string   `json:"timeout"`
	Required    bool     `json:"required"`
	Parser      string   `json:"parser"`
	MinTests    int      `json:"min_tests,omitempty"`
	ReportPath  string   `json:"report_path,omitempty"`
	PassEnv     []string `json:"pass_env,omitempty"`
	FailLevel   string   `json:"fail_level,omitempty"`
	Property    string   `json:"property,omitempty"`
	Scope       string   `json:"scope,omitempty"`
	Assumptions []string `json:"assumptions,omitempty"`
}
type File struct {
	Path   string `json:"path"`
	Mode   uint32 `json:"mode"`
	SHA256 string `json:"sha256"`
	Size   int64  `json:"size"`
}
type Snapshot struct {
	Schema      string `json:"schema"`
	ID          string `json:"id"`
	Repository  string `json:"repository"`
	SourceRef   string `json:"source_ref"`
	Commit      string `json:"commit,omitempty"`
	Files       []File `json:"files"`
	CreatedAt   string `json:"created_at"`
	Consistency string `json:"consistency"`
}
type Change struct {
	Path     string `json:"path"`
	Status   string `json:"status"`
	Category string `json:"category"`
}
type Inspection struct {
	Schema    string    `json:"schema"`
	Base      Snapshot  `json:"base"`
	Candidate Snapshot  `json:"candidate"`
	Changes   []Change  `json:"changes"`
	Findings  []Finding `json:"findings"`
}
type Finding struct {
	Rule     string `json:"rule"`
	Message  string `json:"message"`
	Path     string `json:"path,omitempty"`
	Source   string `json:"source"`
	Severity string `json:"severity,omitempty"`
	Line     int    `json:"line,omitempty"`
	Evidence string `json:"evidence,omitempty"`
}
type ProcessResult struct {
	StartedAt    string `json:"started_at"`
	FinishedAt   string `json:"finished_at"`
	ExitCode     int    `json:"exit_code"`
	Error        string `json:"error,omitempty"`
	TimedOut     bool   `json:"timed_out"`
	Cancelled    bool   `json:"cancelled"`
	Truncated    bool   `json:"truncated"`
	StdoutSHA256 string `json:"stdout_sha256"`
	StderrSHA256 string `json:"stderr_sha256"`
	StdoutBytes  int64  `json:"stdout_bytes"`
	StderrBytes  int64  `json:"stderr_bytes"`
}
type CheckResult struct {
	Parser           string        `json:"parser"`
	ID               string        `json:"id"`
	Required         bool          `json:"required"`
	Outcome          string        `json:"outcome"`
	Meaning          string        `json:"meaning"`
	Tests            int           `json:"tests"`
	Skipped          int           `json:"skipped"`
	Process          ProcessResult `json:"process"`
	SpecDigest       string        `json:"spec_digest"`
	Candidate        string        `json:"candidate"`
	Executable       string        `json:"executable"`
	ExecutableSHA256 string        `json:"executable_sha256,omitempty"`
	EvidenceSource   string        `json:"evidence_source"`
	ReportSHA256     string        `json:"report_sha256,omitempty"`
	Findings         []Finding     `json:"findings,omitempty"`
	Property         string        `json:"property,omitempty"`
	Scope            string        `json:"scope,omitempty"`
	Assumptions      []string      `json:"assumptions,omitempty"`
}
type Investigation struct {
	ExecutionOrder []string          `json:"execution_order,omitempty"`
	Strategy       string            `json:"strategy,omitempty"`
	Schema         string            `json:"schema"`
	ID             string            `json:"id"`
	RoverVersion   string            `json:"rover_version"`
	StartedAt      string            `json:"started_at"`
	FinishedAt     string            `json:"finished_at"`
	Repository     string            `json:"repository"`
	Base           string            `json:"base"`
	Candidate      string            `json:"candidate"`
	ConfigDigest   string            `json:"config_digest"`
	PolicySource   string            `json:"policy_source"`
	Executor       string            `json:"executor"`
	Trust          string            `json:"trust"`
	Environment    map[string]string `json:"environment"`
	Checks         []CheckResult     `json:"checks"`
	Findings       []Finding         `json:"findings"`
	Unknowns       []string          `json:"unknowns"`
	Decision       string            `json:"decision"`
	DecisionReason string            `json:"decision_reason"`
}
type TaskSpec struct {
	Schema     string   `json:"schema"`
	Objective  string   `json:"objective"`
	Repository string   `json:"repository"`
	Base       string   `json:"base"`
	Argv       []string `json:"argv"`
	Timeout    string   `json:"timeout"`
	PassEnv    []string `json:"pass_env,omitempty"`
	DependsOn  []string `json:"depends_on,omitempty"`
	AutoVerify bool     `json:"auto_verify"`
	// Explicitly supplied config is copied before launch. No candidate-written
	// config is loaded after an agent starts.
	ConfigPath      string       `json:"config_path,omitempty"`
	Agent           string       `json:"agent,omitempty"`
	AgentOptions    AgentOptions `json:"agent_options,omitempty"`
	Interactive     bool         `json:"interactive,omitempty"`
	MaxAttempts     int          `json:"max_attempts,omitempty"`
	RepairArgv      []string     `json:"repair_argv,omitempty"`
	Reservations    []string     `json:"reservations,omitempty"`
	InitialSnapshot string       `json:"initial_snapshot,omitempty"`
}
type TaskRun struct {
	Schema          string         `json:"schema"`
	ID              string         `json:"id"`
	Contract        TaskSpec       `json:"contract"`
	ContractDigest  string         `json:"contract_digest"`
	FrozenConfig    *Config        `json:"frozen_config,omitempty"`
	PolicySource    string         `json:"policy_source,omitempty"`
	BaseSnapshot    string         `json:"base_snapshot"`
	Status          string         `json:"status"`
	Workspace       string         `json:"workspace"`
	Candidate       string         `json:"candidate,omitempty"`
	InvestigationID string         `json:"investigation_id,omitempty"`
	CreatedAt       string         `json:"created_at"`
	UpdatedAt       string         `json:"updated_at"`
	Heartbeat       string         `json:"heartbeat,omitempty"`
	PID             int            `json:"pid,omitempty"`
	ProcessIdentity string         `json:"process_identity,omitempty"`
	CancelRequested bool           `json:"cancel_requested"`
	Process         *ProcessResult `json:"process,omitempty"`
	Error           string         `json:"error,omitempty"`
	Attempts        []Attempt      `json:"attempts,omitempty"`
	AgentResult     *AgentResult   `json:"agent_result,omitempty"`
	Socket          string         `json:"socket,omitempty"`
}

func Terminal(s string) bool {
	switch s {
	case "CANDIDATE_READY", "REVIEW_READY", "CHECKS_BLOCKED", "FAILED", "ERROR", "CANCELLED", "TIMED_OUT", "LOST":
		return true
	}
	return false
}

type Approval struct {
	Schema          string `json:"schema"`
	ID              string `json:"id"`
	InvestigationID string `json:"investigation_id"`
	Candidate       string `json:"candidate"`
	ConfigDigest    string `json:"config_digest"`
	At              string `json:"at"`
	Note            string `json:"note"`
	Kind            string `json:"kind"`
	// Local review is a same-user assertion, not authenticated team approval.
	Authority string `json:"authority"`
}
type Capabilities struct {
	Name                string `json:"name"`
	Launch              bool   `json:"launch"`
	Cancel              bool   `json:"cancel"`
	LogFollow           bool   `json:"log_follow"`
	InteractivePTY      bool   `json:"interactive_pty"`
	NativeResume        bool   `json:"native_resume"`
	PermissionMediation bool   `json:"permission_mediation"`
	UsageReporting      bool   `json:"usage_reporting"`
	Status              string `json:"status"`
}

// AgentOptions is an explicit invocation policy, not a universal provider API.
type AgentOptions struct {
	Executable   string   `json:"executable,omitempty"`
	Model        string   `json:"model,omitempty"`
	Write        bool     `json:"write,omitempty"`
	AllowedTools []string `json:"allowed_tools,omitempty"`
	MaxTurns     int      `json:"max_turns,omitempty"`
}
type AgentResult struct {
	Adapter          string           `json:"adapter"`
	Session          string           `json:"session,omitempty"`
	Completed        bool             `json:"completed"`
	Error            string           `json:"error,omitempty"`
	Claims           []string         `json:"claims,omitempty"`
	Usage            map[string]int64 `json:"usage,omitempty"`
	EstimatedCostUSD *float64         `json:"estimated_cost_usd,omitempty"`
	UnknownEvents    int              `json:"unknown_events"`
	Provenance       string           `json:"provenance"`
}
type Attempt struct {
	Number         int           `json:"number"`
	Process        ProcessResult `json:"process"`
	Candidate      string        `json:"candidate,omitempty"`
	Investigation  string        `json:"investigation,omitempty"`
	Agent          *AgentResult  `json:"agent,omitempty"`
	FeedbackSHA256 string        `json:"feedback_sha256,omitempty"`
}
