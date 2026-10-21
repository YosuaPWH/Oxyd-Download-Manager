//! Windows: a named pipe that only accepts local clients, with a DACL that grants access to
//! the owner (the user running the app) and SYSTEM only.

use std::io;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

/// Owner rights + SYSTEM, protected from inheritance.
const SDDL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

struct SecurityAttributes {
    sa: windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
}

impl SecurityAttributes {
    fn new() -> io::Result<Self> {
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
        let wide: Vec<u16> = SDDL.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `wide` is NUL terminated; `sd` receives a LocalAlloc'd descriptor that is
        // released in `Drop`.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(SecurityAttributes {
            sa: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: 0,
            },
        })
    }
}

impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by ConvertStringSecurityDescriptor...W.
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.sa.lpSecurityDescriptor);
        }
    }
}

fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let mut attrs = SecurityAttributes::new()?;
    let mut opts = ServerOptions::new();
    opts.first_pipe_instance(first)
        .reject_remote_clients(true)
        .max_instances(64)
        .in_buffer_size(64 * 1024)
        .out_buffer_size(64 * 1024);
    // SAFETY: `attrs` is a valid SECURITY_ATTRIBUTES that lives across the call.
    unsafe {
        opts.create_with_security_attributes_raw(
            name,
            &mut attrs.sa as *mut _ as *mut std::ffi::c_void,
        )
    }
}

pub struct Listener {
    name: String,
    next: NamedPipeServer,
}

/// Bind the pipe. Fails if another process already owns `name` (e.g. a running instance).
pub fn bind(name: &str) -> io::Result<Listener> {
    Ok(Listener {
        name: name.to_string(),
        next: create(name, true)?,
    })
}

impl Listener {
    /// Wait for the next client. A fresh pipe instance is created for the one after it.
    pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        self.next.connect().await?;
        let fresh = create(&self.name, false)?;
        Ok(std::mem::replace(&mut self.next, fresh))
    }
}
