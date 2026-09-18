use std::{
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd},
};

const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;
const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

/// Filesystem rights handled by the Landlock policy.
#[repr(C)]
#[derive(Debug)]
pub struct RulesetAttr {
    handled_access_fs: u64,
}

/// Rights granted beneath an opened filesystem object.
#[repr(C)]
#[derive(Debug)]
pub struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

/// A prepared, fail-closed Linux worker sandbox.
#[derive(Debug)]
pub struct WorkerSandbox {
    ruleset: File,
}

impl WorkerSandbox {
    /// Builds the filesystem policy before forking the worker.
    pub fn new() -> io::Result<Self> {
        // SAFETY: The version query has no pointer arguments.
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<RulesetAttr>(),
                0,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        if abi < 3 {
            // Earlier ABIs do not mediate truncation, so they cannot enforce no writes.
            return Err(io::Error::new(io::ErrorKind::Unsupported, "Landlock ABI 3 is required"));
        }
        let handled_access_fs = if abi >= 5 { (1 << 16) - 1 } else { (1 << 15) - 1 };
        let attr = RulesetAttr { handled_access_fs };
        // SAFETY: `attr` points to an initialized ruleset attribute for the duration of the call.
        let fd = unsafe {
            libc::syscall(libc::SYS_landlock_create_ruleset, &attr, size_of::<RulesetAttr>(), 0)
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: The successful syscall returned a new owned descriptor.
        let ruleset = unsafe { File::from_raw_fd(fd as i32) };
        let sandbox = Self { ruleset };
        for path in ["/usr", "/lib", "/lib64"] {
            match File::open(path) {
                Ok(file) => sandbox.allow_path(&file, handled_access_fs, true)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        let cache = File::open("/etc/ld.so.cache")?;
        sandbox.allow_path(&cache, handled_access_fs, false)?;
        Ok(sandbox)
    }

    /// Grants only runtime read and execute rights beneath an opened path.
    pub fn allow_path(&self, path: &File, handled: u64, directory: bool) -> io::Result<()> {
        let mut allowed = LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_READ_FILE;
        if directory {
            allowed |= LANDLOCK_ACCESS_FS_READ_DIR;
        }
        allowed &= handled
            & !(LANDLOCK_ACCESS_FS_REFER
                | LANDLOCK_ACCESS_FS_TRUNCATE
                | LANDLOCK_ACCESS_FS_IOCTL_DEV);
        let attr = PathBeneathAttr { allowed_access: allowed, parent_fd: path.as_raw_fd() };
        // SAFETY: Both descriptors and `attr` remain valid throughout the call.
        let result = unsafe {
            libc::syscall(
                libc::SYS_landlock_add_rule,
                self.ruleset.as_raw_fd(),
                LANDLOCK_RULE_PATH_BENEATH,
                &attr,
                0,
            )
        };
        if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// Installs Landlock and seccomp in the post-fork child immediately before `exec`.
    ///
    /// # Safety
    /// Must only be called from a `Command::pre_exec` closure.
    pub unsafe fn install(&self) -> io::Result<()> {
        // Close any inherited database/network descriptors at exec, but keep the sealed
        // executable and Command's error pipe usable until exec itself succeeds.
        // SAFETY: This only marks descriptors in the post-fork child, with no pointer arguments.
        cvt(unsafe { libc::close_range(3, u32::MAX, libc::CLOSE_RANGE_CLOEXEC as i32) })?;
        // SAFETY: This prctl operation takes only scalar arguments and affects this child.
        cvt(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) })?;
        // SAFETY: The owned ruleset descriptor remains open until exec.
        cvt(unsafe {
            libc::syscall(libc::SYS_landlock_restrict_self, self.ruleset.as_raw_fd(), 0) as i32
        })?;
        // SAFETY: We are in the post-fork child with NO_NEW_PRIVS successfully installed.
        unsafe { install_seccomp() }
    }
}

/// Converts a syscall return value into a standard I/O result.
pub fn cvt(result: i32) -> io::Result<()> {
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Installs the worker's process and network syscall filter.
///
/// # Safety
/// Must only run in a worker child with `NO_NEW_PRIVS` already set.
pub unsafe fn install_seccomp() -> io::Result<()> {
    const LD_SYSCALL: libc::sock_filter = libc::sock_filter { code: 0x20, jt: 0, jf: 0, k: 0 };
    const DENY: libc::sock_filter = libc::sock_filter {
        code: 0x06,
        jt: 0,
        jf: 0,
        k: libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
    };
    const ALLOW: libc::sock_filter =
        libc::sock_filter { code: 0x06, jt: 0, jf: 0, k: libc::SECCOMP_RET_ALLOW };
    const DENIED: &[libc::c_long] = &[
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_sendmsg,
        libc::SYS_sendmmsg,
        libc::SYS_recvfrom,
        libc::SYS_recvmsg,
        libc::SYS_recvmmsg,
        libc::SYS_shutdown,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_kill,
        libc::SYS_tkill,
        libc::SYS_tgkill,
        libc::SYS_pidfd_open,
        libc::SYS_pidfd_getfd,
        libc::SYS_pidfd_send_signal,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_kexec_load,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
        libc::SYS_io_uring_setup,
    ];
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    let mut filters = [ALLOW; 96];
    // Reject alternate syscall ABIs, including x32 on x86-64, instead of letting them
    // bypass the native syscall deny list.
    filters[0] = libc::sock_filter { code: 0x20, jt: 0, jf: 0, k: 4 };
    filters[1] = libc::sock_filter { code: 0x15, jt: 1, jf: 0, k: ARCH };
    filters[2] = libc::sock_filter { code: 0x06, jt: 0, jf: 0, k: libc::SECCOMP_RET_KILL_PROCESS };
    filters[3] = LD_SYSCALL;
    filters[4] = libc::sock_filter { code: 0x35, jt: 0, jf: 1, k: 0x40000000 };
    filters[5] = DENY;
    for (index, syscall) in DENIED.iter().enumerate() {
        filters[index * 2 + 6] = libc::sock_filter { code: 0x15, jt: 0, jf: 1, k: *syscall as u32 };
        filters[index * 2 + 7] = DENY;
    }
    let end = DENIED.len() * 2 + 6;
    filters[end] = ALLOW;
    let program = libc::sock_fprog { len: (end + 1) as u16, filter: filters.as_mut_ptr() };
    // SAFETY: Both program and its initialized filter array live throughout the syscall.
    cvt(unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) })
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::process::CommandExt, process::Command};

    use super::*;

    #[test]
    fn child_cannot_access_host_files_network_or_processes() {
        let path =
            std::env::temp_dir().join(format!("history-sandbox-test-{}", std::process::id()));
        fs::write(&path, b"canonical state").unwrap();
        let inherited = File::open(&path).unwrap();
        // SAFETY: This test deliberately makes a descriptor inheritable to test the exec barrier.
        assert_eq!(unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_SETFD, 0) }, 0);
        let sandbox = WorkerSandbox::new().unwrap();
        let mut command = Command::new("/usr/bin/python3");
        command.args([
            "-I",
            "-S",
            "-c",
            r#"
import ctypes, errno, os, socket, sys
def denied(action):
    try:
        action()
    except OSError as error:
        assert error.errno in (errno.EPERM, errno.EACCES, errno.EBADF), error
    else:
        raise AssertionError('sandbox allowed forbidden operation')
denied(lambda: open(sys.argv[1], 'rb'))
denied(lambda: open(sys.argv[1], 'wb'))
denied(lambda: os.read(int(sys.argv[2]), 1))
denied(lambda: open('/proc/' + sys.argv[3] + '/status', 'rb'))
denied(lambda: socket.socket())
libc = ctypes.CDLL(None, use_errno=True)
assert libc.ptrace(0, 0, 0, 0) == -1
assert ctypes.get_errno() == errno.EPERM
print('filesystem, inherited descriptor, proc, socket, ptrace: denied')
"#,
        ]);
        command
            .arg(&path)
            .arg(inherited.as_raw_fd().to_string())
            .arg(std::process::id().to_string());
        // SAFETY: All policy state is constructed above; installation only uses syscalls.
        unsafe { command.pre_exec(move || sandbox.install()) };
        let result = command.output().unwrap();
        let unchanged = fs::read(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(unchanged, b"canonical state");
    }
}
