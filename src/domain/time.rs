use jiff::tz::Offset;
use jiff::Timestamp;

const NANOS_PER_MILLISECOND: i32 = 1_000_000;
const NANOS_PER_MICROSECOND: i32 = 1_000;

/// Formats an instant as the RFC 3339 text the API and the alerts publish.
///
/// The fraction of a second is written with 0, 3, 6 or 9 digits, whichever is
/// the shortest that loses nothing: consumers have always read it in groups of
/// three, trailing zeros included (`.120`, never `.12`).
///
/// # Arguments
///
/// * `at` - The instant to format.
///
/// # Returns
///
/// The instant in UTC with a numeric offset, e.g. `2026-01-02T03:04:05+00:00`.
pub fn rfc3339_utc(at: Timestamp) -> String {
    let nanos = at.subsec_nanosecond();
    let fraction_digits = if nanos == 0 {
        0
    } else if nanos % NANOS_PER_MILLISECOND == 0 {
        3
    } else if nanos % NANOS_PER_MICROSECOND == 0 {
        6
    } else {
        9
    };
    // `{:.*}` takes the precision from an argument instead of a literal.
    format!(
        "{:.*}",
        fraction_digits,
        at.display_with_offset(Offset::UTC)
    )
}
