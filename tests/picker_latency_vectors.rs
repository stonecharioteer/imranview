#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerCause {
    DirectoryLookupBound,
    NativeDialogOrUserBlocking,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PickerEvent {
    lookup_ms: u128,
    blocked_ms: u128,
    selected: bool,
}

fn parse_ms(line: &str, marker: &str) -> Option<u128> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find("ms")?;
    rest[..end].trim().parse::<u128>().ok()
}

fn parse_picker_events(log: &str) -> Vec<PickerEvent> {
    let mut events = Vec::new();
    let mut pending_lookup_ms: Option<u128> = None;

    for line in log.lines() {
        if line.contains("open picker prepare") && line.contains("lookup=") {
            pending_lookup_ms = parse_ms(line, "lookup=");
            continue;
        }

        if line.contains("open picker selected") && line.contains("blocked=") {
            if let (Some(lookup_ms), Some(blocked_ms)) =
                (pending_lookup_ms.take(), parse_ms(line, "blocked="))
            {
                events.push(PickerEvent {
                    lookup_ms,
                    blocked_ms,
                    selected: true,
                });
            }
            continue;
        }

        if line.contains("open picker cancelled") && line.contains("blocked=") {
            if let (Some(lookup_ms), Some(blocked_ms)) =
                (pending_lookup_ms.take(), parse_ms(line, "blocked="))
            {
                events.push(PickerEvent {
                    lookup_ms,
                    blocked_ms,
                    selected: false,
                });
            }
        }
    }

    events
}

fn classify(event: &PickerEvent) -> PickerCause {
    if event.lookup_ms >= 100 && event.lookup_ms.saturating_mul(4) >= event.blocked_ms {
        return PickerCause::DirectoryLookupBound;
    }

    if event.blocked_ms >= 750 && event.lookup_ms <= 20 {
        return PickerCause::NativeDialogOrUserBlocking;
    }

    PickerCause::Mixed
}

#[test]
fn classifies_lookup_bound_when_lookup_is_large_fraction_of_blocking() {
    let fixture = r#"
[2026-04-06T16:10:00.000Z DEBUG imranview::ui] open picker prepare preferred_directory=/tmp lookup=650ms
[2026-04-06T16:10:01.100Z DEBUG imranview::ui] open picker selected path=/tmp/a.jpg blocked=1100ms
"#;

    let events = parse_picker_events(fixture);
    assert_eq!(events.len(), 1);
    assert_eq!(classify(&events[0]), PickerCause::DirectoryLookupBound);
}

#[test]
fn classifies_native_or_user_blocking_when_lookup_is_tiny_but_blocking_is_large() {
    let fixture = r#"
[2026-04-06T16:28:19.984Z DEBUG imranview::ui] open picker prepare preferred_directory=/Users/me/Pictures/wallpapers/phone lookup=0ms
[2026-04-06T16:28:23.856Z DEBUG imranview::ui] open picker selected path=/Users/me/Pictures/wallpapers/phone/x.jpg blocked=3872ms
"#;

    let events = parse_picker_events(fixture);
    assert_eq!(events.len(), 1);
    assert_eq!(classify(&events[0]), PickerCause::NativeDialogOrUserBlocking);
}

#[test]
fn parses_cancelled_picker_events_and_classifies_them() {
    let fixture = r#"
[2026-04-06T16:42:24.799Z DEBUG imranview::ui] open picker prepare preferred_directory=/Users/me/Pictures/wallpapers/phone lookup=0ms
[2026-04-06T16:42:27.376Z DEBUG imranview::ui] open picker cancelled blocked=2576ms
"#;

    let events = parse_picker_events(fixture);
    assert_eq!(events.len(), 1);
    assert!(!events[0].selected);
    assert_eq!(classify(&events[0]), PickerCause::NativeDialogOrUserBlocking);
}
