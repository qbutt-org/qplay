use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::core::PCWSTR;

struct ChildJob(HANDLE);

impl ChildJob {
    fn attach(child: &std::process::Child) -> Result<Self, String> {
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| format!("Own helper process: {error}"))?;
        let job = Self(handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .map_err(|error| format!("Limit helper process lifetime: {error}"))?;
        unsafe { AssignProcessToJobObject(handle, HANDLE(child.as_raw_handle())) }
            .map_err(|error| format!("Attach helper process: {error}"))?;
        Ok(job)
    }
}

impl Drop for ChildJob {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Run on a worker; the caller cancels when the media generation changes.
pub fn run_bounded(
    program: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    timeout: Duration,
    max_output_bytes: u64,
    environment: Option<(&str, &Path)>,
    error_output: Option<&mut Vec<u8>>,
) -> Result<Vec<u8>, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("The helper was cancelled.".into());
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(if error_output.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    if let Some((name, value)) = environment {
        command.env(name, value);
    }
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .spawn()
        .map_err(|error| format!("Start helper process: {error}"))?;
    let _job = match ChildJob::attach(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let mut output = child.stdout.take().ok_or("The helper has no output.")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .by_ref()
            .take(max_output_bytes + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let error_reader = child.stderr.take().map(|mut stream| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer)?;
                if count == 0 {
                    return Ok::<_, std::io::Error>(bytes);
                }
                let remaining = 4096 - bytes.len();
                bytes.extend_from_slice(&buffer[..count.min(remaining)]);
            }
        })
    });
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire) || started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            if let Some(error_reader) = error_reader {
                let _ = error_reader.join();
            }
            return Err("The helper was cancelled or timed out.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                if let Some(error_reader) = error_reader {
                    let _ = error_reader.join();
                }
                return Err(format!("Wait for helper process: {error}"));
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Read helper output: reader stopped unexpectedly.")?
        .map_err(|error| format!("Read helper output: {error}"))?;
    if let (Some(error_reader), Some(error_output)) = (error_reader, error_output) {
        *error_output = error_reader
            .join()
            .map_err(|_| "Read helper error output: reader stopped unexpectedly.")?
            .map_err(|error| format!("Read helper error output: {error}"))?;
    }
    if !status.success() || bytes.len() as u64 > max_output_bytes {
        return Err(format!("The helper exited with status {status}."));
    }
    Ok(bytes)
}
