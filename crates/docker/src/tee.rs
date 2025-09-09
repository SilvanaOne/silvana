#[cfg(target_os = "linux")]
use crate::error::Result;
#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
#[cfg(target_os = "linux")]
use std::io::Write;

#[cfg(target_os = "linux")]
pub async fn cleanup_resources() -> Result<()> {
    drop_page_cache()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn drop_page_cache() -> std::io::Result<()> {
    unsafe { libc::sync() };
    
    let mut f = OpenOptions::new()
        .write(true)
        .open("/proc/sys/vm/drop_caches")?;
    f.write_all(b"3")?;
    
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub async fn cleanup_resources() -> Result<()> {
    Ok(())
}