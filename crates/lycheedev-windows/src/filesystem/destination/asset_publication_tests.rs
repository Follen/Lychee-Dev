use super::*;
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
struct Control(Arc<AtomicBool>);
impl FileControl for Control {
    type Error = io::Error;
    fn charge(&mut self, _: FileCost) -> io::Result<()> {
        self.check()
    }
    fn check(&mut self) -> io::Result<()> {
        if self.0.load(Ordering::Acquire) {
            Err(io::Error::other("fixture.cancelled"))
        } else {
            Ok(())
        }
    }
}
struct Body {
    raw: io::Cursor<Vec<u8>>,
    reads: usize,
    cancel: Option<Arc<AtomicBool>>,
}
impl Read for Body {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.reads += 1;
        let n = self.raw.read(out)?;
        if n != 0 {
            if let Some(c) = self.cancel.take() {
                c.store(true, Ordering::Release)
            }
        }
        Ok(n)
    }
}
fn root() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "lychee-asset-publish-{}",
        crate::random::token().unwrap()
    ));
    fs::create_dir(&p).unwrap();
    p
}
#[test]
fn controlled_asset_stage_reads_body_once_and_consumes_all_handles() {
    let root = root();
    let path = root.join("out");
    let mut control = Control(Arc::new(AtomicBool::new(false)));
    let destination = Destination::prepare_controlled(&path, false, &mut control).unwrap();
    destination.check_controlled(&mut control).unwrap();
    let mut body = Body {
        raw: io::Cursor::new(vec![7; 65537]),
        reads: 0,
        cancel: None,
    };
    let result = destination.publish_reader_controlled(&mut body, 65537, 65537, &mut control);
    assert!(result.result.is_ok());
    assert!(result.cleanup.is_empty());
    assert_eq!(result.publication, StagedPublication::Published);
    assert_eq!(body.reads, 3);
    destination.close().unwrap();
    assert_eq!(fs::read(&path).unwrap(), vec![7; 65537]);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn observed_source_cancel_discards_only_private_stage_without_replay() {
    let root = root();
    let path = root.join("out");
    let token = Arc::new(AtomicBool::new(false));
    let mut control = Control(token.clone());
    let destination = Destination::prepare_controlled(&path, false, &mut control).unwrap();
    let mut body = Body {
        raw: io::Cursor::new(vec![1; 7]),
        reads: 0,
        cancel: Some(token),
    };
    let result = destination.publish_reader_controlled(&mut body, 7, 7, &mut control);
    assert_eq!(result.result.unwrap_err().to_string(), "fixture.cancelled");
    assert_eq!(result.publication, StagedPublication::NotPublished);
    assert!(result.cleanup.is_empty());
    assert_eq!(body.reads, 1);
    destination.close().unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn short_or_long_source_never_publishes_or_leaves_private_stage() {
    let root = root();
    for (declared, kind) in [
        (2, io::ErrorKind::InvalidInput),
        (4, io::ErrorKind::UnexpectedEof),
    ] {
        let path = root.join(format!("out-{declared}"));
        let mut control = Control(Arc::new(AtomicBool::new(false)));
        let destination = Destination::prepare_controlled(&path, false, &mut control).unwrap();
        let result = destination.publish_reader_controlled(
            &mut io::Cursor::new(b"abc"),
            declared,
            10,
            &mut control,
        );
        assert_eq!(result.result.unwrap_err().kind(), kind);
        assert!(result.cleanup.is_empty());
        assert_eq!(result.publication, StagedPublication::NotPublished);
        destination.close().unwrap();
        assert!(!path.exists());
    }
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}
