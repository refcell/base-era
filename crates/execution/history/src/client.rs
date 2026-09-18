use std::{
    fs::{self, File},
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use alloy_primitives::{Address, B256, U256};
use history_worker_protocol::{
    ExecuteRequest, Outcome, ReadRequest, ReadResponse, RpcError, RpcSuccess, VERSION,
};
use revm::Database;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::WorkerSandbox;

const MAX_FRAME: usize = 64 * 1024 * 1024;
const WORKER_TIMEOUT: Duration = Duration::from_secs(30);

/// An out-of-band approved worker and frozen chain configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerManifest {
    /// Worker executable path.
    pub executable: PathBuf,
    /// Approved executable digest.
    pub executable_sha256: String,
    /// Exact canonical genesis value passed to the worker.
    pub genesis: Value,
    /// Identity of the canonical genesis JSON.
    pub genesis_identity: String,
    /// Consensus hash of the genesis header.
    pub genesis_header_hash: String,
    /// Identity of the frozen effective configuration.
    pub config_identity: String,
    /// Decimal chain identifier.
    pub chain_id: String,
}

/// Historical worker host client.
#[derive(Clone, Debug)]
pub struct HistoryWorker {
    /// Approved executable and immutable chain inputs.
    pub manifest: WorkerManifest,
}

/// A fail-closed worker or protocol failure.
#[derive(Debug, thiserror::Error)]
pub enum HistoryWorkerError {
    /// Approved artifact does not match the executable.
    #[error("worker artifact digest mismatch")]
    ArtifactMismatch,
    /// Worker reported invalid consensus input.
    #[error("historical execution rejected the block: {0}")]
    Invalid(String),
    /// Worker cannot execute this valid request.
    #[error("historical execution is unsupported: {0}")]
    Unsupported(String),
    /// Worker infrastructure failed. This must never be mapped to block invalidity.
    #[error("historical execution infrastructure failure: {0}")]
    Infrastructure(String),
    /// The pinned worker returned an RPC error.
    #[error("historical RPC error {code}: {message}")]
    Rpc {
        /// JSON-RPC error code preserved from historical execution.
        code: i32,
        /// Era-correct error message.
        message: String,
        /// Optional revert data or structured error details.
        data: Option<Value>,
    },
}

impl HistoryWorker {
    /// Reads an approved manifest. The path is explicit so configuration cannot silently drift.
    pub fn from_manifest(path: impl AsRef<Path>) -> Result<Self, HistoryWorkerError> {
        let bytes = fs::read(path).map_err(infra)?;
        let manifest = serde_json::from_slice(&bytes).map_err(infra)?;
        Ok(Self { manifest })
    }

    /// Executes one fully-bound request while serving immutable parent-state reads.
    pub fn execute<DB: Database>(
        &self,
        request_id: String,
        era: String,
        parent_header_rlp: String,
        child_block_rlp: String,
        state: &mut DB,
    ) -> Result<Outcome, HistoryWorkerError>
    where
        DB::Error: std::fmt::Display,
    {
        let value =
            self.invoke(request_id.clone(), era, parent_header_rlp, child_block_rlp, None, state)?;
        let outcome = serde_json::from_value::<Outcome>(value).map_err(infra)?;
        match &outcome {
            Outcome::Success { request_id: id, .. } if id == &request_id => Ok(outcome),
            Outcome::InvalidInput { request_id: Some(id), error } if id == &request_id => {
                Err(HistoryWorkerError::Invalid(error.clone()))
            }
            Outcome::Unsupported { request_id: id, error } if id == &request_id => {
                Err(HistoryWorkerError::Unsupported(error.clone()))
            }
            Outcome::Infrastructure { request_id: Some(id), error } if id == &request_id => {
                Err(HistoryWorkerError::Infrastructure(error.clone()))
            }
            _ => Err(infrastructure("terminal outcome is not bound to request")),
        }
    }

    /// Executes a pinned historical RPC operation against host-served state.
    pub fn rpc<DB: Database>(
        &self,
        request_id: String,
        era: String,
        state_header_rlp: String,
        block_rlp: String,
        operation: Value,
        state: &mut DB,
    ) -> Result<Value, HistoryWorkerError>
    where
        DB::Error: std::fmt::Display,
    {
        let value = self.invoke(
            request_id.clone(),
            era,
            state_header_rlp,
            block_rlp,
            Some(operation),
            state,
        )?;
        if let Ok(success) = serde_json::from_value::<RpcSuccess>(value.clone())
            && success.request_id == request_id
        {
            return Ok(success.result);
        }
        if let Ok(error) = serde_json::from_value::<RpcError>(value)
            && error.request_id == request_id
        {
            return Err(HistoryWorkerError::Rpc {
                code: error.code,
                message: error.message,
                data: error.data,
            });
        }
        Err(infrastructure("terminal RPC response is not bound to request"))
    }

    /// Invokes the worker protocol for an execution or RPC request.
    pub fn invoke<DB: Database>(
        &self,
        request_id: String,
        era: String,
        parent_header_rlp: String,
        child_block_rlp: String,
        operation: Option<Value>,
        state: &mut DB,
    ) -> Result<Value, HistoryWorkerError>
    where
        DB::Error: std::fmt::Display,
    {
        let executable = self.verified_executable()?;
        let sandbox = WorkerSandbox::new().map_err(infra)?;
        let executable_path = format!("/proc/self/fd/{}", executable.as_raw_fd());
        let mut command = Command::new(executable_path);
        command.env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // SAFETY: `install` performs only syscalls over policy memory prepared before `fork`.
        unsafe { command.pre_exec(move || sandbox.install()) };
        let child = command.spawn().map_err(infra)?;
        let worker_pid = child.id();
        let deadline = Instant::now() + WORKER_TIMEOUT;
        let mut guard = ChildGuard::new(child);
        let mut request = ExecuteRequest {
            version: VERSION,
            request_id: request_id.clone(),
            executable_sha256: self.manifest.executable_sha256.clone(),
            worker_pid,
            binding_hash: String::new(),
            era,
            chain_id: self.manifest.chain_id.clone(),
            genesis_identity: self.manifest.genesis_identity.clone(),
            genesis_header_hash: self.manifest.genesis_header_hash.clone(),
            config_identity: self.manifest.config_identity.clone(),
            genesis: self.manifest.genesis.clone(),
            parent_header_rlp,
            child_block_rlp,
            operation,
        };
        request.binding_hash = format!(
            "{:#x}",
            alloy_primitives::keccak256(request.binding_payload_bytes().map_err(infra)?)
        );
        let started = Instant::now();
        tracing::info!(request = %request_id, worker_pid, era = %request.era, operation = ?request.operation, artifact = %request.executable_sha256, configuration = %request.config_identity, binding = %request.binding_hash, "historical worker execution started");
        let stdin =
            guard.child_mut().stdin.take().ok_or_else(|| infrastructure("missing worker stdin"))?;
        let stdout = guard
            .child_mut()
            .stdout
            .take()
            .ok_or_else(|| infrastructure("missing worker stdout"))?;
        Self::set_nonblocking(&stdin)?;
        Self::set_nonblocking(&stdout)?;
        Self::write_frame(&stdin, &request, deadline)?;
        let mut sequence = 1;
        let mut read_bytes = 0usize;
        let outcome = loop {
            let (value, frame_bytes): (Value, usize) = Self::read_frame(&stdout, deadline)?;
            if let Ok(read) = serde_json::from_value::<ReadRequest>(value.clone()) {
                read_bytes = read_bytes.saturating_add(frame_bytes);
                let (id, seq) = Self::read_binding(&read);
                if id != request_id || seq != sequence {
                    return Err(infrastructure("unbound or out-of-order state read"));
                }
                let value = Self::serve_read(state, read)?;
                Self::write_frame(
                    &stdin,
                    &ReadResponse { request_id: request_id.clone(), sequence, value, error: None },
                    deadline,
                )?;
                sequence = sequence
                    .checked_add(1)
                    .ok_or_else(|| infrastructure("read sequence overflow"))?;
                continue;
            }
            break value;
        };
        drop(stdin);
        let status = guard.wait(deadline)?;
        if !status.success() {
            return Err(infrastructure("worker exited unsuccessfully"));
        }
        tracing::info!(request = %request_id, worker_pid, operation = ?request.operation, elapsed_us = started.elapsed().as_micros(), requests = sequence - 1, read_bytes, "historical worker execution completed");
        Ok(outcome)
    }

    /// Copies the approved worker bytes into a sealed executable memory file.
    pub fn verified_executable(&self) -> Result<File, HistoryWorkerError> {
        let bytes = fs::read(&self.manifest.executable).map_err(infra)?;
        let digest = format!("0x{}", hex::encode(Sha256::digest(&bytes)));
        if digest != self.manifest.executable_sha256 {
            return Err(HistoryWorkerError::ArtifactMismatch);
        }

        // The command executes this exact verified byte sequence, not the mutable manifest path.
        let name = b"base-history-worker\0";
        // SAFETY: `name` is NUL-terminated and the syscall has no pointer output parameters.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_memfd_create,
                name.as_ptr().cast::<libc::c_char>(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        if fd < 0 {
            return Err(infra(std::io::Error::last_os_error()));
        }
        // SAFETY: the successful syscall returned a new descriptor owned by this function.
        let mut executable = unsafe { File::from_raw_fd(fd as i32) };
        executable.write_all(&bytes).map_err(infra)?;
        let seals =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        // SAFETY: F_ADD_SEALS operates on the valid memfd and does not access userspace memory.
        if unsafe { libc::fcntl(executable.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
            return Err(infra(std::io::Error::last_os_error()));
        }
        Ok(executable)
    }

    /// Serves one worker state read from the provided database.
    pub fn serve_read<DB: Database>(
        state: &mut DB,
        read: ReadRequest,
    ) -> Result<Option<Value>, HistoryWorkerError>
    where
        DB::Error: std::fmt::Display,
    {
        Ok(match read {
            ReadRequest::Account { address, .. } => {
                match state.basic(address.parse::<Address>().map_err(infra)?).map_err(infra)? {
                    None => None,
                    Some(account) => {
                        let code = match account.code {
                            Some(code) => code,
                            None => state.code_by_hash(account.code_hash).map_err(infra)?,
                        };
                        Some(json!({
                            "nonce": account.nonce.to_string(), "balance": account.balance.to_string(),
                            "code": format!("0x{}", hex::encode(code.original_bytes())),
                        }))
                    }
                }
            }
            ReadRequest::Code { code_hash, .. } => Some(json!(format!(
                "0x{}",
                hex::encode(
                    state
                        .code_by_hash(code_hash.parse::<B256>().map_err(infra)?)
                        .map_err(infra)?
                        .original_bytes()
                )
            ))),
            ReadRequest::Storage { address, key, .. } => Some(json!(format!(
                "{:#066x}",
                state
                    .storage(address.parse().map_err(infra)?, key.parse::<U256>().map_err(infra)?)
                    .map_err(infra)?
            ))),
            ReadRequest::BlockHash { number, .. } => Some(json!(format!(
                "{:#x}",
                state.block_hash(number.parse().map_err(infra)?).map_err(infra)?
            ))),
        })
    }
}

/// Owns a worker child and guarantees it is reaped when execution stops early.
#[derive(Debug)]
pub struct ChildGuard(Option<Child>);

impl ChildGuard {
    /// Creates a guard for a running worker child.
    pub const fn new(child: Child) -> Self {
        Self(Some(child))
    }

    /// Returns the guarded child.
    pub const fn child_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect("child present")
    }

    /// Waits until the child exits or the absolute deadline is reached.
    pub fn wait(
        &mut self,
        deadline: Instant,
    ) -> Result<std::process::ExitStatus, HistoryWorkerError> {
        loop {
            if let Some(status) =
                self.0.as_mut().expect("child present").try_wait().map_err(infra)?
            {
                self.0.take();
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(infrastructure("worker exit timeout"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl HistoryWorker {
    /// Waits for a descriptor event until an absolute deadline.
    pub fn wait_ready(
        fd: &impl AsRawFd,
        events: libc::c_short,
        deadline: Instant,
    ) -> Result<(), HistoryWorkerError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(infrastructure("worker transport timeout"));
        }
        let mut pollfd = libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 };
        let timeout = remaining.as_millis().max(1).min(i32::MAX as u128) as i32;
        // SAFETY: `fd` points to one initialized pollfd for the duration of this call.
        let result = unsafe { libc::poll(&mut pollfd, 1, timeout) };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Self::wait_ready(fd, events, deadline);
            }
            return Err(infra(error));
        }
        if result == 0 {
            return Err(infrastructure("worker transport timeout"));
        }
        if pollfd.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(infrastructure("worker pipe failed"));
        }
        Ok(())
    }

    /// Marks a worker transport descriptor as nonblocking.
    pub fn set_nonblocking(fd: &impl AsRawFd) -> Result<(), HistoryWorkerError> {
        // SAFETY: F_GETFL does not modify memory and `fd` remains owned by the caller.
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(infra(std::io::Error::last_os_error()));
        }
        // SAFETY: F_SETFL updates flags on the valid descriptor owned by the caller.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(infra(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Returns the request identity and sequence bound to a state read.
    pub fn read_binding(read: &ReadRequest) -> (&str, u32) {
        match read {
            ReadRequest::Account { request_id, sequence, .. }
            | ReadRequest::Code { request_id, sequence, .. }
            | ReadRequest::Storage { request_id, sequence, .. }
            | ReadRequest::BlockHash { request_id, sequence, .. } => (request_id, *sequence),
        }
    }

    /// Writes one length-delimited protocol frame by an absolute deadline.
    pub fn write_frame(
        writer: &impl AsRawFd,
        value: &impl Serialize,
        deadline: Instant,
    ) -> Result<(), HistoryWorkerError> {
        let body = serde_json::to_vec(value).map_err(infra)?;
        if body.len() > MAX_FRAME {
            return Err(infrastructure("frame exceeds 64 MiB"));
        }
        let mut frame = Vec::with_capacity(4 + body.len());
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);
        Self::write_all_deadline(writer, &frame, deadline)
    }

    /// Reads one length-delimited protocol frame by an absolute deadline.
    pub fn read_frame<T: DeserializeOwned>(
        reader: &impl AsRawFd,
        deadline: Instant,
    ) -> Result<(T, usize), HistoryWorkerError> {
        let mut prefix = [0; 4];
        Self::read_exact_deadline(reader, &mut prefix, deadline)?;
        let len = u32::from_be_bytes(prefix) as usize;
        if len > MAX_FRAME {
            return Err(infrastructure("frame exceeds 64 MiB"));
        }
        let mut body = vec![0; len];
        Self::read_exact_deadline(reader, &mut body, deadline)?;
        Ok((serde_json::from_slice(&body).map_err(infra)?, len + prefix.len()))
    }

    /// Fills a buffer from a descriptor by an absolute deadline.
    pub fn read_exact_deadline(
        reader: &impl AsRawFd,
        mut buffer: &mut [u8],
        deadline: Instant,
    ) -> Result<(), HistoryWorkerError> {
        while !buffer.is_empty() {
            Self::wait_ready(reader, libc::POLLIN, deadline)?;
            // SAFETY: `buffer` is writable for its reported length and the fd remains owned by reader.
            let read =
                unsafe { libc::read(reader.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
            if read == 0 {
                return Err(infrastructure("worker closed response pipe mid-frame"));
            }
            if read < 0 {
                let error = std::io::Error::last_os_error();
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) {
                    continue;
                }
                return Err(infra(error));
            }
            buffer = &mut buffer[read as usize..];
        }
        Ok(())
    }

    /// Writes a complete buffer to a descriptor by an absolute deadline.
    pub fn write_all_deadline(
        writer: &impl AsRawFd,
        mut buffer: &[u8],
        deadline: Instant,
    ) -> Result<(), HistoryWorkerError> {
        while !buffer.is_empty() {
            Self::wait_ready(writer, libc::POLLOUT, deadline)?;
            // SAFETY: `buffer` is readable for its reported length and the fd remains owned by writer.
            let written =
                unsafe { libc::write(writer.as_raw_fd(), buffer.as_ptr().cast(), buffer.len()) };
            if written == 0 {
                return Err(infrastructure("worker request pipe made no progress"));
            }
            if written < 0 {
                let error = std::io::Error::last_os_error();
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) {
                    continue;
                }
                return Err(infra(error));
            }
            buffer = &buffer[written as usize..];
        }
        Ok(())
    }
}

/// Converts a displayable failure into an infrastructure failure.
pub fn infra(error: impl std::fmt::Display) -> HistoryWorkerError {
    infrastructure(&error.to_string())
}
/// Creates an infrastructure failure with a stable message.
pub fn infrastructure(error: &str) -> HistoryWorkerError {
    HistoryWorkerError::Infrastructure(error.to_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        os::{fd::AsRawFd, unix::net::UnixStream},
        time::{Duration, Instant},
    };

    use serde_json::Value;
    use sha2::{Digest, Sha256};

    use super::{HistoryWorker, HistoryWorkerError, WorkerManifest};

    #[test]
    fn partial_frame_obeys_absolute_deadline() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        HistoryWorker::set_nonblocking(&reader).unwrap();
        writer.write_all(&10_u32.to_be_bytes()).unwrap();
        writer.write_all(b"{").unwrap();

        let started = Instant::now();
        let error =
            HistoryWorker::read_frame::<Value>(&reader, started + Duration::from_millis(50))
                .unwrap_err();

        assert!(matches!(error, HistoryWorkerError::Infrastructure(_)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn malformed_response_is_infrastructure_failure() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        HistoryWorker::set_nonblocking(&reader).unwrap();
        writer.write_all(&1_u32.to_be_bytes()).unwrap();
        writer.write_all(b"{").unwrap();

        let error =
            HistoryWorker::read_frame::<Value>(&reader, Instant::now() + Duration::from_secs(1))
                .unwrap_err();

        assert!(matches!(error, HistoryWorkerError::Infrastructure(_)));
    }

    #[test]
    fn wrong_artifact_is_rejected_before_memfd_creation() {
        let worker = test_worker("0x00".into());

        assert!(matches!(worker.verified_executable(), Err(HistoryWorkerError::ArtifactMismatch)));
    }

    #[test]
    fn verified_executable_is_fully_sealed() {
        let bytes = fs::read(std::env::current_exe().unwrap()).unwrap();
        let worker = test_worker(format!("0x{}", hex::encode(Sha256::digest(bytes))));
        let executable = worker.verified_executable().unwrap();

        // SAFETY: F_GET_SEALS only reads metadata from the valid memfd.
        let seals = unsafe { libc::fcntl(executable.as_raw_fd(), libc::F_GET_SEALS) };
        assert_eq!(
            seals,
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
        );
    }

    fn test_worker(executable_sha256: String) -> HistoryWorker {
        HistoryWorker {
            manifest: WorkerManifest {
                executable: std::env::current_exe().unwrap(),
                executable_sha256,
                genesis: Value::Null,
                genesis_identity: String::new(),
                genesis_header_hash: String::new(),
                config_identity: String::new(),
                chain_id: String::new(),
            },
        }
    }
}
