//! Text formatting in SpaceMonger's style, plus local-time conversion
//! without any date/time crate.

use std::path::{MAIN_SEPARATOR, Path};
use std::time::Duration;

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Human-readable bytes using Windows-style labels and powers of 1024.
/// Keep one truncated decimal above bytes, matching SpaceMonger's size values.
pub fn size(bytes: u64) -> String {
    const UNITS: [(u64, &str); 6] =
        [(1 << 60, "EB"), (1 << 50, "PB"), (1 << 40, "TB"), (1 << 30, "GB"), (1 << 20, "MB"), (1 << 10, "KB")];
    for (unit, name) in UNITS {
        if bytes >= unit {
            let full = bytes / unit;
            let tenth = (10 * u128::from(bytes % unit) / u128::from(unit)) as u64;
            return format!("{full}.{tenth} {name}");
        }
    }
    format!("{bytes} B")
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
struct CivilTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// Break seconds-since-epoch into a UTC calendar time
/// (Howard Hinnant's `civil_from_days`).
fn utc_time(secs: i64) -> CivilTime {
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
fn local_time(secs: i64) -> CivilTime {
    utc_time(secs + local_offset(secs).unwrap_or(0))
}

/// Windows FILETIMEs count 100 ns ticks since 1601-01-01 UTC.
const FILETIME_TICKS_PER_SECOND: i64 = 10_000_000;
const FILETIME_UNIX_EPOCH_SECONDS: i64 = 11_644_473_600;

/// Whole seconds since the Unix epoch for a FILETIME tick count.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn unix_from_filetime(ticks: u64) -> Option<i64> {
    Some(i64::try_from(ticks / FILETIME_TICKS_PER_SECOND as u64).ok()? - FILETIME_UNIX_EPOCH_SECONDS)
}

#[cfg(windows)]
fn filetime_from_unix(secs: i64) -> Option<u64> {
    u64::try_from(secs.checked_add(FILETIME_UNIX_EPOCH_SECONDS)?.checked_mul(FILETIME_TICKS_PER_SECOND)?).ok()
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
    let ticks = filetime_from_unix(secs)?;
    let local = crate::windows::local_filetime(ticks)?;
    Some((local as i64 - ticks as i64) / FILETIME_TICKS_PER_SECOND)
}

#[cfg(not(any(unix, windows)))]
fn local_offset(_secs: i64) -> Option<i64> {
    None
}

/// `part` as a share of `whole`, capped at 1; nothing of nothing is 0.
pub fn fraction(part: u64, whole: u64) -> f32 {
    if whole == 0 { 0.0 } else { (part as f64 / whole as f64).min(1.0) as f32 }
}

/// The last path component, or the whole path for a drive root.
pub fn display_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// A folder path with a trailing separator, as SpaceMonger titled folders.
pub fn dir_display(p: &Path) -> String {
    let mut s = p.display().to_string();
    if !s.ends_with(MAIN_SEPARATOR) {
        s.push(MAIN_SEPARATOR);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_display_has_trailing_separator() {
        assert!(dir_display(Path::new("/a/b")).ends_with(MAIN_SEPARATOR));
    }

    #[test]
    fn fractions_are_capped_and_empty_wholes_are_zero() {
        assert_eq!(fraction(1, 4).to_bits(), 0.25_f32.to_bits());
        assert_eq!(fraction(5, 4).to_bits(), 1.0_f32.to_bits());
        assert_eq!(fraction(5, 0).to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn readable_sizes_keep_binary_values() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(1024), "1.0 KB");
        assert_eq!(size(1535), "1.4 KB"); // truncated, not rounded
        assert_eq!(size(5 * 1024 * 1024 + 1024 * 1023), "5.9 MB");
        assert_eq!(size(3 << 30), "3.0 GB");
        assert_eq!(size(2 << 40), "2.0 TB");
        assert_eq!(size(94_983_340_321), "88.4 GB");
        assert_eq!(size(1 << 50), "1.0 PB");
        assert_eq!(size(u64::MAX), "15.9 EB");
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
    }

    #[test]
    fn filetimes_convert_to_unix_seconds() {
        assert_eq!(unix_from_filetime(116_444_736_000_000_000), Some(0));
        assert_eq!(unix_from_filetime(0), Some(-FILETIME_UNIX_EPOCH_SECONDS));
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
