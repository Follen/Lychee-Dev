//! Read-only current process memory headroom. No allocation or policy decisions.
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

pub fn available_memory() -> crate::Result<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status is an aligned, initialized MEMORYSTATUSEX of dwLength
    // bytes, exclusively borrowed for this synchronous call. The API writes
    // only that structure and retains no pointer. No handle or thread affinity
    // is involved. Read fields only after the API reports success.
    unsafe { GlobalMemoryStatusEx(&mut status) }.map_err(|error| error.to_string())?;
    Ok(available(&status))
}

fn available(status: &MEMORYSTATUSEX) -> u64 {
    // AvailablePageFile is process commit capacity, including process/job
    // limits, rather than the machine-wide page-file size.
    status
        .ullAvailPhys
        .min(status.ullAvailPageFile)
        .min(status.ullAvailVirtual)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_uses_each_available_field_and_not_totals() {
        for values in [[3, 5, 7], [7, 3, 5], [5, 7, 3], [0, 5, 7]] {
            let status = MEMORYSTATUSEX {
                ullAvailPhys: values[0],
                ullAvailPageFile: values[1],
                ullAvailVirtual: values[2],
                ..Default::default()
            };
            assert_eq!(available(&status), *values.iter().min().unwrap());
        }
    }
    #[test]
    fn windows_query_memory_uses_native_availability() {
        assert_eq!(std::mem::size_of::<MEMORYSTATUSEX>(), 64);
        assert!(available_memory().unwrap() > 0);
    }
}
