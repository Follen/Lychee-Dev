package memory

import "strings"

type QualificationLevel string

const (
	QualificationUnknown QualificationLevel = "unidentified"
	QualificationL0      QualificationLevel = "L0"
	QualificationL1      QualificationLevel = "L1"
	QualificationL2      QualificationLevel = "L2"
	QualificationL3      QualificationLevel = "L3"
	QualificationL4      QualificationLevel = "L4"
)

// DuplexABI names an implemented layout: locating an RVA cannot choose guessed
// write offsets for another client.
type DuplexABI struct {
	ID               string `json:"id"`
	TableArrayOffset uint32 `json:"tableArrayOffset"`
	TableCountOffset uint32 `json:"tableCountOffset"`
	TValueStride     uint32 `json:"tvalueStride"`
	NumberTag        byte   `json:"numberTag"`
	TagOffset        uint32 `json:"tagOffset"`
	SecretOffset     uint32 `json:"secretOffset"`
	FreezeFlagOffset uint32 `json:"freezeFlagOffset"`
	FrozenFlagValue  byte   `json:"frozenFlagValue"`
	NumberEncoding   string `json:"numberEncoding"`
	FreezeSemantics  string `json:"freezeSemantics"`
}

var retailDuplexABI = DuplexABI{
	ID: "retail-69933-tvalue24-v1", TableArrayOffset: 0x20, TableCountOffset: 0x40,
	TValueStride: duplexValueBytes, NumberTag: 3, TagOffset: 8, SecretOffset: 9,
	FreezeFlagOffset: 0x45, FrozenFlagValue: 1,
	NumberEncoding: "ieee754-binary64-le-u32-exact", FreezeSemantics: "addon-frozen-row-strong-private-roots-v1",
}

type DuplexTrialEvidence struct {
	Mode        string   `json:"mode"`
	RealClient  bool     `json:"realClient"`
	CLICommit   string   `json:"cliCommit"`
	AddonCommit string   `json:"addonCommit"`
	Record      string   `json:"record"`
	Passed      []string `json:"passed"`
}

type DuplexWriterProfile struct {
	ID                 string              `json:"id"`
	Mode               string              `json:"mode"`
	ExecutableSHA256   string              `json:"executableSHA256"`
	Build              string              `json:"build"`
	Product            string              `json:"product"`
	RecipeID           string              `json:"recipeId"`
	RecipeVersion      uint32              `json:"recipeVersion"`
	ABI                DuplexABI           `json:"abi"`
	LifecycleID        string              `json:"lifecycleId"`
	LayoutEvidence     string              `json:"layoutEvidence"`
	LifetimeEvidence   string              `json:"lifetimeEvidence"`
	Validation         string              `json:"validation"`
	ImageQualification QualificationLevel  `json:"imageQualification"`
	Trial              DuplexTrialEvidence `json:"trial"`
	Gaps               []string            `json:"gaps"`
}

// Image evidence is reusable only for this exact product/full-build/hash.
// Every process still rechecks roots, rows, actor and lifecycle before WPM.
// L3 is restricted use, not L4 acceptance.
func DuplexWriteCapability(executableSHA256, build, product string) DuplexWriterProfile {
	p := DuplexWriterProfile{ExecutableSHA256: executableSHA256, Build: build, Product: product, Validation: "not_run", ImageQualification: QualificationUnknown, Gaps: []string{"L2 exact-image ABI and lifecycle evidence", "L3 real-client direct trial", "L4 scenario acceptance"}}
	if validImageIdentity(executableSHA256, build, product) {
		p.ImageQualification = QualificationL0
	}
	if executableSHA256 == RetailLuaMailboxExecutableSHA256 && build == retailReloadSourceBuild && product == "retail" {
		p.ID, p.Mode = "retail-69933-direct-mailbox-v1-trial", "direct"
		p.RecipeID, p.RecipeVersion = LuaMailboxRootRecipeID, 1
		p.ABI, p.LifecycleID = retailDuplexABI, RetailReloadRecipeID
		p.LayoutEvidence = "matching runtime .text static evidence; numeric calibration, capacity and frozen row rechecked for each write"
		p.LifetimeEvidence = "strong Lua roots pin ordinary GC; exact direct trial passed; final-check-to-reload race remains"
		p.Validation, p.ImageQualification = "retail_small_and_1mib_verified", QualificationL3
		p.Trial = DuplexTrialEvidence{Mode: "direct", RealClient: true, CLICommit: "39844a4", AddonCommit: "b021251", Record: "docs/toolkit/live-mailbox-v1-direct-retail-trial-2026-10-04.md", Passed: []string{"small_command", "1mib_command", "whole_row_readback", "execution_result", "ack_close"}}
		p.Gaps = []string{"cancel", "fault_recovery", "reload_and_actor", "same_build_multi_instance", "performance_and_memory", "final_check_to_reload_race"}
	}
	return p
}

func validImageIdentity(hash, build, product string) bool {
	return len(hash) == 64 && strings.Trim(hash, "0123456789abcdef") == "" && build != "" && product != ""
}

// CanDirectWrite derives authority from an implemented ABI and exact-image
// trial evidence. A recipe or successful calibration alone cannot promote it.
func (p DuplexWriterProfile) CanDirectWrite() bool {
	if !p.hasWriterConditions() || p.ImageQualification != QualificationL3 || !p.Trial.RealClient || p.Trial.Mode != "direct" || p.Trial.CLICommit == "" || p.Trial.AddonCommit == "" || p.Trial.Record == "" {
		return false
	}
	for _, required := range []string{"small_command", "1mib_command", "whole_row_readback", "execution_result", "ack_close"} {
		found := false
		for _, passed := range p.Trial.Passed {
			if passed == required {
				found = true
			}
		}
		if !found {
			return false
		}
	}
	return true
}

func (p DuplexWriterProfile) hasWriterConditions() bool {
	return p.ExecutableSHA256 == RetailLuaMailboxExecutableSHA256 && p.Build == retailReloadSourceBuild && p.Product == "retail" && p.Mode == "direct" && p.RecipeID == LuaMailboxRootRecipeID && p.RecipeVersion == 1 && p.ABI == retailDuplexABI && p.LifecycleID == RetailReloadRecipeID && (p.ImageQualification == QualificationL2 || p.ImageQualification == QualificationL3) && p.LayoutEvidence != "" && p.LifetimeEvidence != ""
}

type DuplexProfileObservation struct {
	ProcessIdentified bool
	RootRecipeID      string
	TypedMailbox      bool
	LifecycleID       string
	NumericRow        bool
}

type DuplexQualification struct {
	Level       QualificationLevel `json:"level"`
	DirectWrite bool               `json:"directWrite"`
	Missing     []string           `json:"missing"`
}

// L1 permits a unique read-only recipe on a new image; it never inherits L2.
// This diagnostic does not replace the publisher's immediate runtime gates.
func (p DuplexWriterProfile) Qualify(o DuplexProfileObservation) DuplexQualification {
	q := DuplexQualification{Level: QualificationUnknown}
	if !o.ProcessIdentified || !validImageIdentity(p.ExecutableSHA256, p.Build, p.Product) {
		q.Missing = []string{"L0 exact process and image identity"}
		return q
	}
	q.Level = QualificationL0
	if (o.RootRecipeID != LuaMailboxRootRecipeID && o.RootRecipeID != ForeverLuaMailboxRootRecipeID) || !o.TypedMailbox || o.LifecycleID != RetailReloadRecipeID {
		q.Missing = []string{"L1 unique root recipe, typed mailbox and independent lifecycle"}
		return q
	}
	q.Level = QualificationL1
	if !p.hasWriterConditions() || o.RootRecipeID != p.RecipeID || o.LifecycleID != p.LifecycleID || !o.NumericRow {
		q.Missing = []string{"L2 exact-image ABI, numeric/frozen row and lifetime evidence", "L3 exact-image direct trial"}
		return q
	}
	q.Level = QualificationL2
	if !p.CanDirectWrite() {
		q.Missing = []string{"L3 exact-image direct trial"}
		return q
	}
	q.Level, q.DirectWrite = QualificationL3, true
	q.Missing = append([]string(nil), p.Gaps...)
	return q
}
