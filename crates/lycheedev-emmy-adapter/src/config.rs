//! Pinned-engine host configuration. This API intentionally exposes Emmyrc
//! inside the engine-host boundary; business records and worker DTOs stay owned.
use emmylua_code_analysis::{Emmyrc, EmmyrcLuaVersion};

pub fn lua51_config() -> Emmyrc {
    let mut config = Emmyrc::default();
    config.runtime.version = EmmyrcLuaVersion::Lua51;
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lua51_host_config_preserves_complete_original_serialization() {
        let mut previous = Emmyrc::default();
        previous.runtime.version = EmmyrcLuaVersion::Lua51;
        assert_eq!(
            serde_json::to_vec(&lua51_config()).unwrap(),
            serde_json::to_vec(&previous).unwrap()
        );
    }
}
