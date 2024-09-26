use crate::*;

pub fn friendly_duration(remaining: Duration) -> String {
    let days = remaining.as_secs() / 86400;
    let remaining = remaining - Duration::from_secs(86400 * days);
    let hours = remaining.as_secs() / 3600;
    let remaining = remaining - Duration::from_secs(3600 * hours);
    let mins = remaining.as_secs() / 60;
    let remaining = remaining - Duration::from_secs(60 * mins);
    let secs = remaining.as_secs();
    if days > 0 {
        format!("{}d {:02}:{:02}:{:02}", days, hours, mins, secs)
    } else {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    }
}

pub fn trim_newline(s: &mut String) {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
}