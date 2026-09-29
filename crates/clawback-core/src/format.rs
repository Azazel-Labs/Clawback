//! Text formatting in SpaceMonger's style, plus local-time conversion
//! without any date/time crate.

use std::time::Duration;

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `"12.3 Gb"`: binary units, one truncated decimal (SpaceMonger's `size_format`).
pub fn size(bytes: u64) -> String {
    const UNITS: [(u64, &str); 4] = [(1 << 40, "Tb"), (1 << 30, "Gb"), (1 << 20, "Mb"), (1 << 10, "Kb")];
    for (unit, name) in UNITS {
        if bytes >= unit {
            let full = bytes / unit;
            let tenth = (10 * u128::from(bytes % unit) / u128::from(unit)) as u64;
            return format!("{full}.{tenth} {name}");
        }
    }
    format!("{bytes}.0 bytes")
}

/// `"12.3%"`, truncated like the original.
pub fn percent(part: u64, whole: u64) -> String {
    let whole = if whole == 0 { u64::MAX } else { whole };
    let permille = (u128::from(part) * 1000 / u128::from(whole)) as u64;
    format!("{}.{}%", permille / 10, permille % 10)
}

/// `"1,234,567"`.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `"1,234,567 bytes"`.
pub fn bytes_exact(n: u64) -> String {
    format!("{} bytes", count(n))
}

/// `"29 Sep 2026   13:25:07"` in local time (empty if unknown).
pub fn date(secs: i64) -> String {
    if secs == i64::MIN {
        return String::new();
    }
    let t = local_time(secs);
    format!(
        "{:02} {} {:04}   {}:{:02}:{:02}",
        t.day,
        MONTHS[(t.month - 1) as usize % 12],
        t.year,
        t.hour,
        t.minute,
        t.second
    )
}

/// `"1.4 s"`, `"2 min 05 s"`.
pub fn duration(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s < 60.0 {
        format!("{s:.1} s")
    } else {
        let s = d.as_secs();
        format!("{} min {:02} s", s / 60, s % 60)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// Break seconds-since-epoch into a UTC calendar time
/// (Howard Hinnant's `civil_from_days`).
pub fn utc_time(secs: i64) -> CivilTime {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    CivilTime {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        minute: (rem % 3600 / 60) as u32,
        second: (rem % 60) as u32,
    }
}

/// Convert to the machine's local time zone (falls back to UTC).
pub fn local_time(secs: i64) -> CivilTime {
    utc_time(secs + local_offset(secs).unwrap_or(0))
}

#[cfg(unix)]
fn local_offset(secs: i64) -> Option<i64> {
    use std::os::raw::{c_char, c_int, c_long};
    // Enough room for every libc's `struct tm`; we only read the leading
    // fields, which POSIX fixes, and `tm_gmtoff`, which glibc, musl, macOS
    // and the BSDs all place right after them.
    #[repr(C)]
    struct Tm {
        fields: [c_int; 9],
        gmtoff: c_long,
        zone: *const c_char,
        _spare: [u8; 64],
    }
    unsafe extern "C" {
        fn localtime_r(t: *const c_long, out: *mut Tm) -> *mut Tm;
    }
    let t = c_long::try_from(secs).ok()?;
    let mut tm = Tm { fields: [0; 9], gmtoff: 0, zone: std::ptr::null(), _spare: [0; 64] };
    // SAFETY: `tm` is a valid, writable buffer at least as large as `struct tm`.
    let r = unsafe { localtime_r(&raw const t, &raw mut tm) };
    (!r.is_null()).then_some(tm.gmtoff)
}

#[cfg(windows)]
fn local_offset(secs: i64) -> Option<i64> {
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToLocalFileTime(utc: *const FileTime, local: *mut FileTime) -> i32;
    }
    let ticks = u64::try_from((secs + 11_644_473_600).checked_mul(10_000_000)?).ok()?;
    let utc = FileTime { low: ticks as u32, high: (ticks >> 32) as u32 };
    let mut local = FileTime { low: 0, high: 0 };
    // SAFETY: both pointers refer to valid FILETIME structs.
    if unsafe { FileTimeToLocalFileTime(&raw const utc, &raw mut local) } == 0 {
        return None;
    }
    let lt = (u64::from(local.high) << 32) | u64::from(local.low);
    Some((lt as i64 - ticks as i64) / 10_000_000)
}

#[cfg(not(any(unix, windows)))]
fn local_offset(_secs: i64) -> Option<i64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_like_spacemonger() {
        assert_eq!(size(0), "0.0 bytes");
        assert_eq!(size(1023), "1023.0 bytes");
        assert_eq!(size(1024), "1.0 Kb");
        assert_eq!(size(1535), "1.4 Kb"); // truncated, not rounded
        assert_eq!(size(5 * 1024 * 1024 + 1024 * 1023), "5.9 Mb");
        assert_eq!(size(3 << 30), "3.0 Gb");
        assert_eq!(size(2 << 40), "2.0 Tb");
    }

    #[test]
    fn percents_and_counts() {
        assert_eq!(percent(1, 3), "33.3%");
        assert_eq!(percent(5, 0), "0.0%");
        assert_eq!(percent(10, 10), "100.0%");
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(bytes_exact(12345), "12,345 bytes");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(utc_time(0), CivilTime { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 });
        // 2026-09-29 17:25:07 UTC
        assert_eq!(
            utc_time(1_790_702_707),
            CivilTime { year: 2026, month: 9, day: 29, hour: 17, minute: 25, second: 7 }
        );
        assert_eq!(utc_time(-1), CivilTime { year: 1969, month: 12, day: 31, hour: 23, minute: 59, second: 59 });
        assert_eq!(utc_time(951_782_400).day, 29); // 2000-02-29
        assert!(date(i64::MIN).is_empty());
        assert!(date(0).contains("19")); // local 1969 or 1970
    }
}
