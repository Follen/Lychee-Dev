//! Opt-in main-entry diagnostics, not process startup or accepted command time.
//! A separate flag keeps the existing Data budget recorder's framing unchanged.
use std::{io::Write, time::Instant};

const TIMING: &str = "wall-clock from instrumented main entry; excludes pre-main startup and this CLI profile emission; dispatch includes nested Data phases and Data profile emission; external elapsed remainder is unattributed, not startup or CPU time";
const DESCRIBE_TIMING: &str = "wall-clock from instrumented main entry; parse includes argument collection, catalog and command parsing; dispatch builds the complete result; render includes envelope, encoding, output and flush; excludes pre-main startup, this profile emission and later destruction; external remainder is unattributed";

pub(super) struct Profile {
    started: Option<Instant>,
    parsed: Option<Instant>,
    dispatch: Option<Instant>,
    render: Option<Instant>,
    command: Option<&'static str>,
    describe_enabled: bool,
}
impl Profile {
    pub(super) fn from_env() -> Self {
        let describe_enabled =
            std::env::var_os("LYCHEEDEV_CLI_PHASE_PROFILE").is_some_and(|v| v == "1");
        let mut profile = Self::new(
            describe_enabled
                || std::env::var_os("LYCHEEDEV_DATA_CLI_PROFILE").is_some_and(|v| v == "1"),
            Instant::now,
        );
        profile.describe_enabled = describe_enabled;
        profile
    }
    fn new(enabled: bool, now: impl FnOnce() -> Instant) -> Self {
        Self {
            started: enabled.then(now),
            parsed: None,
            dispatch: None,
            render: None,
            command: None,
            describe_enabled: false,
        }
    }
    pub(super) fn parsed(&mut self) {
        if self.describe_enabled {
            self.parsed = Some(Instant::now());
        }
    }
    pub(super) fn dispatch(&mut self, command: Option<&str>) {
        if self.started.is_none() {
            return;
        }
        self.command = match command {
            Some("data cache prepare") => Some("data cache prepare"),
            Some("describe") if self.describe_enabled => Some("describe"),
            _ => None,
        };
        if self.command.is_some() {
            self.dispatch = Some(Instant::now());
        }
    }
    pub(super) fn render(&mut self) {
        if self.command.is_some() {
            self.render = Some(Instant::now());
        }
    }
    fn value(&self, end: impl FnOnce() -> Instant, succeeded: bool) -> Option<serde_json::Value> {
        let (Some(start), Some(dispatch), Some(render), Some(command)) =
            (self.started, self.dispatch, self.render, self.command)
        else {
            return None;
        };
        let end = end();
        if command == "describe" {
            let parsed = self.parsed?;
            return Some(serde_json::json!({
                "schema":"lycheedev.cli-phases.v1", "timing":DESCRIBE_TIMING, "command":command,
                "parseMicros":parsed.duration_since(start).as_micros(),
                "parseToDispatchMicros":dispatch.duration_since(parsed).as_micros(),
                "dispatchMicros":render.duration_since(dispatch).as_micros(),
                "renderMicros":end.duration_since(render).as_micros(),
                "mainElapsedMicros":end.duration_since(start).as_micros(),
                "renderSucceeded":succeeded,
            }));
        }
        Some(serde_json::json!({
            "schema":"lycheedev.data-cli-phases.v1", "timing":TIMING, "command":command,
            "mainEntryToDispatchMicros":dispatch.duration_since(start).as_micros(),
            "dispatchMicros":render.duration_since(dispatch).as_micros(),
            "renderMicros":end.duration_since(render).as_micros(),
            "mainElapsedMicros":end.duration_since(start).as_micros(),
            "renderSucceeded":succeeded,
        }))
    }
    pub(super) fn finish(&self, succeeded: bool) {
        if let Some(value) = self.value(Instant::now, succeeded) {
            let _ = writeln!(std::io::stderr().lock(), "{value}");
        }
    }
    #[cfg(test)]
    fn finish_to(&self, output: &mut impl Write, succeeded: bool) {
        if let Some(value) = self.value(Instant::now, succeeded) {
            // Observation failure cannot replace output/business failures.
            let _ = writeln!(output, "{value}");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn disabled_cli_profile_never_reads_clock_or_writes() {
        let mut profile = Profile::new(false, || panic!("disabled clock"));
        profile.dispatch(Some("data cache prepare"));
        profile.render();
        assert!(profile.value(|| panic!("disabled elapsed"), true).is_none());
        let mut bytes = Vec::new();
        profile.finish_to(&mut bytes, true);
        assert!(bytes.is_empty());
    }
    #[test]
    fn cli_profile_boundaries_and_failed_render_are_exact() {
        let start = Instant::now();
        let profile = Profile {
            started: Some(start),
            parsed: None,
            dispatch: Some(start + Duration::from_micros(3)),
            render: Some(start + Duration::from_micros(10)),
            command: Some("data cache prepare"),
            describe_enabled: false,
        };
        let value = profile
            .value(|| start + Duration::from_micros(15), false)
            .unwrap();
        assert_eq!(value["mainEntryToDispatchMicros"], 3);
        assert_eq!(value["dispatchMicros"], 7);
        assert_eq!(value["renderMicros"], 5);
        assert_eq!(value["mainElapsedMicros"], 15);
        assert_eq!(value["renderSucceeded"], false);
        assert_eq!(value["timing"], TIMING);
        let mut unrelated = Profile::new(true, || start);
        unrelated.dispatch(Some("source query"));
        unrelated.render();
        assert!(
            unrelated
                .value(|| panic!("unrelated clock"), true)
                .is_none()
        );
    }
    #[test]
    fn describe_profile_keeps_parse_dispatch_and_render_separate() {
        let start = Instant::now();
        let profile = Profile {
            started: Some(start),
            parsed: Some(start + Duration::from_micros(2)),
            dispatch: Some(start + Duration::from_micros(3)),
            render: Some(start + Duration::from_micros(10)),
            command: Some("describe"),
            describe_enabled: true,
        };
        let value = profile
            .value(|| start + Duration::from_micros(15), true)
            .unwrap();
        assert_eq!(value["schema"], "lycheedev.cli-phases.v1");
        assert_eq!(value["parseMicros"], 2);
        assert_eq!(value["parseToDispatchMicros"], 1);
        assert_eq!(value["dispatchMicros"], 7);
        assert_eq!(value["renderMicros"], 5);
        assert_eq!(value["mainElapsedMicros"], 15);
        let mut data_only = Profile::new(true, || start);
        data_only.parsed();
        data_only.dispatch(Some("describe"));
        data_only.render();
        assert!(data_only.value(|| panic!("unrelated clock"), true).is_none());
    }
    #[test]
    fn cli_profile_closed_diagnostic_sink_does_not_fail_command() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut profile = Profile::new(true, Instant::now);
        profile.dispatch(Some("data cache prepare"));
        profile.render();
        profile.finish_to(&mut Closed, true);
    }
}
