// Adapted from EmmyLuaAnalyzer (MIT), upstream 08b0873962e83bb8307e9a81052e3f41578fbc78.
// Original: crates/emmylua_code_analysis/src/check/builtin.rs
// See THIRD_PARTY_NOTICES.md for the original license and attribution.
//! Lua standard library global names (M0 hardcoded set; replaced later once std lib indexing is integrated).

/// Common Lua global names: treated as defined, avoiding false undefined_global diagnostics.
pub fn is_builtin_global(name: &str) -> bool {
    matches!(
        name,
        "print"
            | "type"
            | "pairs"
            | "ipairs"
            | "next"
            | "select"
            | "rawget"
            | "rawset"
            | "rawlen"
            | "setmetatable"
            | "getmetatable"
            | "require"
            | "pcall"
            | "xpcall"
            | "tostring"
            | "tonumber"
            | "error"
            | "assert"
            | "unpack"
            | "rawequal"
            | "collectgarbage"
            | "dofile"
            | "load"
            | "loadfile"
            | "loadstring"
            | "warn"
            | "table"
            | "string"
            | "math"
            | "os"
            | "io"
            | "coroutine"
            | "utf8"
            | "debug"
            | "_G"
            | "_ENV"
    )
}
