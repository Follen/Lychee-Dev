use super::*;
use lycheedev_core::control::TrackedCancellation;

#[derive(Clone)]
pub(super) struct Lifetime(TrackedCancellation);
impl Lifetime {
    pub(super) fn new() -> Self {
        Self(TrackedCancellation::new())
    }
    pub(super) fn check(&self) -> Result<()> {
        if self.0.is_cancelled() {
            Err(ToolkitError::new(
                "command.cancelled",
                "DD task owner stopped its connection",
                7,
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    pub(super) fn operation(&self) -> Result<OperationControl> {
        self.check()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        Ok(OperationControl::tracked(deadline, self.0.clone()))
    }
    pub(super) fn stop(&self) {
        self.0.cancel();
    }
}
