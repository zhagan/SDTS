use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::protocol::Envelope;

/// Appends every SDTP message for a session as one JSON line to
/// `recordings/<session_id>.jsonl`. Append-only, matching the "immutable
/// events" rule in docs/protocol.md, and giving replay a byte-exact record
/// of what was sent.
pub struct Recorder {
    file: File,
}

impl Recorder {
    pub fn new(session_id: &str) -> io::Result<Self> {
        fs::create_dir_all("recordings")?;
        let path = format!("recordings/{session_id}.jsonl");
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Recorder { file })
    }

    pub fn append(&mut self, envelope: &Envelope) -> io::Result<()> {
        writeln!(self.file, "{}", envelope.to_line())
    }
}

/// A readable, sortable session id derived from wall-clock time, e.g.
/// `20260725-201530-123`.
pub fn new_session_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch");
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    // Minimal manual UTC calendar/time breakdown to avoid pulling in a
    // date/time crate for a filename.
    let days_since_epoch = secs / 86_400;
    let secs_of_day = secs % 86_400;
    let (hour, minute, second) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );
    let (year, month, day) = civil_from_days(days_since_epoch as i64);

    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-{millis:03}")
}

/// Howard Hinnant's civil_from_days algorithm: days-since-epoch -> (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
