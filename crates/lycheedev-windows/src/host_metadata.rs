//! Current-machine OS version and UUID node, without process/account access.
use crate::Result;
use windows::{Win32::System::SystemInformation::OSVERSIONINFOW, core::GUID};

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version: *mut OSVERSIONINFOW) -> i32;
}
#[link(name = "rpcrt4")]
unsafe extern "system" {
    fn UuidCreateSequential(uuid: *mut GUID) -> u32;
}
pub struct HostMetadata {
    pub uuid_node: [u8; 6],
    pub windows_version: [u32; 3],
}
pub fn current() -> Result<HostMetadata> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: this is the complete Windows SDK output structure; its size
    // selects the ABI. RtlGetVersion reports the OS, independent of manifests.
    if unsafe { RtlGetVersion(&mut version) } != 0 {
        return Err("Windows version metadata is unavailable".into());
    }
    let mut uuid = GUID::from_u128(0);
    // SAFETY: RPC owns no returned allocation and writes exactly one GUID.
    // The last six bytes are the same UUID node source used by Python getnode.
    let status = unsafe { UuidCreateSequential(&mut uuid) };
    // RPC_S_UUID_LOCAL_ONLY still returns a valid local node, including machines
    // with no Ethernet address. It is device metadata, never authentication.
    if status != 0 && status != 1824 {
        return Err("Windows UUID node metadata is unavailable".into());
    }
    Ok(HostMetadata {
        uuid_node: uuid.data4[2..].try_into().expect("six-byte UUID node"),
        windows_version: [
            version.dwMajorVersion,
            version.dwMinorVersion,
            version.dwBuildNumber,
        ],
    })
}
