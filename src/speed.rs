//! The drive speed test: writes one temporary file, reads it back, then does small random
//! reads and writes inside it, all with the system's file cache switched off so the numbers
//! are the drive's and not the Mac's memory. The file is deleted when the test ends, is
//! cancelled, or fails.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::FileExt,
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Write,
    Read,
    SmallRead,
    SmallWrite,
}

pub const PHASES: [Phase; 4] = [
    Phase::Write,
    Phase::Read,
    Phase::SmallRead,
    Phase::SmallWrite,
];

impl Phase {
    pub fn doing(self) -> &'static str {
        match self {
            Phase::Write => "Writing a large file",
            Phase::Read => "Reading it back",
            Phase::SmallRead => "Reading small pieces",
            Phase::SmallWrite => "Writing small pieces",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    /// 0 to 1 within the phase.
    pub fraction: f32,
    /// Speed so far in this phase, in MB/s.
    pub mbps: f64,
}

/// Speeds in MB/s (millions of bytes a second, as drive makers quote them).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpeedResult {
    pub write: f64,
    pub read: f64,
    pub small_read: f64,
    pub small_read_iops: f64,
    pub small_write: f64,
    pub small_write_iops: f64,
    pub file_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Plan {
    pub file_bytes: u64,
    /// How long each of the two small-piece phases runs.
    pub small_seconds: f64,
}

const BLOCK: usize = 8 << 20;
const SMALL: usize = 4096;
const GIB: u64 = 1 << 30;

impl Plan {
    /// The largest test that leaves the drive comfortable room, or None when it is too full.
    pub fn for_free_space(free: u64) -> Option<Plan> {
        let file_bytes = if free >= 4 * GIB {
            GIB
        } else if free >= GIB {
            256 << 20
        } else {
            return None;
        };
        Some(Plan {
            file_bytes,
            small_seconds: 3.0,
        })
    }
}

unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}
/// macOS: turn off data caching for this file.
const F_NOCACHE: i32 = 48;

/// Asks macOS not to keep this file's data in memory. If the request fails the test still
/// runs; reads would then look faster than the drive is.
fn uncached(file: &File) {
    if cfg!(target_os = "macos") {
        // SAFETY: fcntl on a file descriptor this process owns, with an integer argument.
        unsafe {
            fcntl(file.as_raw_fd(), F_NOCACHE, 1i32);
        }
    }
}

/// Deletes the test file and its folder when dropped, whatever happened.
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fast generator: good enough to make data a drive can't compress, and to pick offsets.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let bytes = self.next().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }
}

const FOLDER: &str = ".cable-check-speed-test";

fn mb_per_s(bytes: u64, time: Duration) -> f64 {
    bytes as f64 / 1e6 / time.as_secs_f64().max(1e-9)
}

/// Runs the whole test inside `dir`, which must be on the drive to measure.
pub fn run(
    dir: &Path,
    plan: Plan,
    cancel: &AtomicBool,
    mut report: impl FnMut(Progress),
) -> Result<SpeedResult, String> {
    let folder = dir.join(FOLDER);
    // A folder left behind by a crash is ours to clear.
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "macOS didn't allow writing to this drive. If it asked for permission, allow it and try again. The drive may also be read-only.".to_string()
        } else {
            format!("Couldn't create the test file: {e}")
        }
    })?;
    let _cleanup = Cleanup(folder.clone());
    let path = folder.join("test.bin");
    let stopped = || cancel.load(Ordering::Relaxed);
    let failed =
        |what: &str, e: std::io::Error| format!("The drive stopped responding while {what}: {e}");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ plan.file_bytes);
    let mut block = vec![0u8; BLOCK];
    rng.fill(&mut block);
    let mut last_report = Instant::now();
    let mut tick = |phase: Phase, fraction: f32, mbps: f64, force: bool| {
        if force || last_report.elapsed() >= Duration::from_millis(100) {
            last_report = Instant::now();
            report(Progress {
                phase,
                fraction,
                mbps,
            });
        }
    };

    // 1. One large file, start to finish. The final sync is part of the time: until the data
    // is on the drive it hasn't been written.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("Couldn't create the test file: {e}"))?;
    uncached(&file);
    tick(Phase::Write, 0.0, 0.0, true);
    let start = Instant::now();
    let mut done = 0u64;
    while done < plan.file_bytes {
        if stopped() {
            return Err("Cancelled".into());
        }
        let n = BLOCK.min((plan.file_bytes - done) as usize);
        file.write_all(&block[..n])
            .map_err(|e| failed("writing", e))?;
        done += n as u64;
        tick(
            Phase::Write,
            done as f32 / plan.file_bytes as f32,
            mb_per_s(done, start.elapsed()),
            false,
        );
    }
    file.sync_all().map_err(|e| failed("writing", e))?;
    let write = mb_per_s(done, start.elapsed());
    drop(file);

    // 2. Read the same file back.
    let mut file = File::open(&path).map_err(|e| failed("reading", e))?;
    uncached(&file);
    tick(Phase::Read, 0.0, 0.0, true);
    let start = Instant::now();
    let mut done = 0u64;
    loop {
        if stopped() {
            return Err("Cancelled".into());
        }
        let n = file.read(&mut block).map_err(|e| failed("reading", e))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        tick(
            Phase::Read,
            done as f32 / plan.file_bytes as f32,
            mb_per_s(done, start.elapsed()),
            false,
        );
    }
    let read = mb_per_s(done, start.elapsed());
    drop(file);

    // 3 and 4. Small pieces at random places, one at a time: what opening apps and lots of
    // little files feel like.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| failed("reading", e))?;
    uncached(&file);
    let slots = (plan.file_bytes / SMALL as u64).max(1);
    let limit = Duration::from_secs_f64(plan.small_seconds);
    let mut small = vec![0u8; SMALL];

    tick(Phase::SmallRead, 0.0, 0.0, true);
    let start = Instant::now();
    let mut count = 0u64;
    while start.elapsed() < limit {
        if stopped() {
            return Err("Cancelled".into());
        }
        let at = (rng.next() % slots) * SMALL as u64;
        file.read_exact_at(&mut small, at)
            .map_err(|e| failed("reading", e))?;
        count += 1;
        tick(
            Phase::SmallRead,
            (start.elapsed().as_secs_f64() / plan.small_seconds) as f32,
            mb_per_s(count * SMALL as u64, start.elapsed()),
            false,
        );
    }
    let time = start.elapsed();
    let small_read = mb_per_s(count * SMALL as u64, time);
    let small_read_iops = count as f64 / time.as_secs_f64().max(1e-9);

    tick(Phase::SmallWrite, 0.0, 0.0, true);
    rng.fill(&mut small);
    let start = Instant::now();
    let mut count = 0u64;
    while start.elapsed() < limit {
        if stopped() {
            return Err("Cancelled".into());
        }
        let at = (rng.next() % slots) * SMALL as u64;
        file.write_all_at(&small, at)
            .map_err(|e| failed("writing", e))?;
        count += 1;
        tick(
            Phase::SmallWrite,
            (start.elapsed().as_secs_f64() / plan.small_seconds) as f32,
            mb_per_s(count * SMALL as u64, start.elapsed()),
            false,
        );
    }
    file.sync_all().map_err(|e| failed("writing", e))?;
    let time = start.elapsed();
    let small_write = mb_per_s(count * SMALL as u64, time);
    let small_write_iops = count as f64 / time.as_secs_f64().max(1e-9);

    Ok(SpeedResult {
        write,
        read,
        small_read,
        small_read_iops,
        small_write,
        small_write_iops,
        file_bytes: plan.file_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_plan() -> Plan {
        Plan {
            file_bytes: (BLOCK + 12345) as u64,
            small_seconds: 0.05,
        }
    }

    #[test]
    fn measures_every_phase_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let mut seen = vec![];
        let r = run(dir.path(), small_plan(), &AtomicBool::new(false), |p| {
            if seen.last() != Some(&p.phase) {
                seen.push(p.phase);
            }
        })
        .unwrap();
        assert_eq!(seen, PHASES);
        assert!(r.write > 0.0 && r.read > 0.0);
        assert!(r.small_read_iops > 0.0 && r.small_write_iops > 0.0);
        assert_eq!(r.file_bytes, small_plan().file_bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancelling_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let r = run(dir.path(), small_plan(), &cancel, |p| {
            if p.phase == Phase::Read {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(r, Err("Cancelled".into()));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn an_unwritable_place_fails_cleanly() {
        let r = run(
            Path::new("/nonexistent-drive/x"),
            small_plan(),
            &AtomicBool::new(false),
            |_| {},
        );
        assert!(r.is_err());
    }

    #[test]
    fn test_size_follows_free_space() {
        assert_eq!(Plan::for_free_space(100 * GIB).unwrap().file_bytes, GIB);
        assert_eq!(Plan::for_free_space(2 * GIB).unwrap().file_bytes, 256 << 20);
        assert!(Plan::for_free_space(GIB / 2).is_none());
    }
}
