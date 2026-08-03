use std::time::{SystemTime, UNIX_EPOCH};

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const MONTH: i64 = 30 * DAY;
const YEAR: i64 = 365 * DAY;

pub fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

pub fn relative(seconds: i64) -> String {
    relative_to(seconds, now_seconds())
}

fn relative_to(seconds: i64, now: i64) -> String {
    let delta = now - seconds;
    if delta < 0 {
        return "in the future".to_owned();
    }

    let (count, unit) = match delta {
        d if d < MINUTE => return "just now".to_owned(),
        d if d < HOUR => (d / MINUTE, "minute"),
        d if d < DAY => (d / HOUR, "hour"),
        d if d < MONTH => (d / DAY, "day"),
        d if d < YEAR => (d / MONTH, "month"),
        d => (d / YEAR, "year"),
    };

    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_each_unit() {
        let now = 1_000_000_000;
        assert_eq!(relative_to(now, now), "just now");
        assert_eq!(relative_to(now - 59, now), "just now");
        assert_eq!(relative_to(now - MINUTE, now), "1 minute ago");
        assert_eq!(relative_to(now - 5 * MINUTE, now), "5 minutes ago");
        assert_eq!(relative_to(now - HOUR, now), "1 hour ago");
        assert_eq!(relative_to(now - 3 * DAY, now), "3 days ago");
        assert_eq!(relative_to(now - 2 * MONTH, now), "2 months ago");
        assert_eq!(relative_to(now - 4 * YEAR, now), "4 years ago");
    }

    #[test]
    fn handles_clock_skew() {
        assert_eq!(relative_to(100, 0), "in the future");
    }
}
