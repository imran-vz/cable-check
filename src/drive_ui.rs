//! The Drive speed page: pick a drive, run the test, read four numbers.
use crate::State;
use crate::drives::{self, Drive};
use crate::guide::{self, Term};
use crate::speed::{PHASES, Phase, SpeedResult};
use crate::ui::{Paint, body, button, card, eyebrow, hint, link_button, page_head, t};
use rune::{Cursor, El, TextStyle, col, el, row, text};

/// "1,234" above 100, "45.6" below: enough digits to compare, no more.
fn speed(n: f64) -> String {
    if n < 100.0 {
        return format!("{n:.1}");
    }
    let whole = format!("{}", n.round() as u64);
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1000.0 {
        format!("{:.1} TB", gb / 1000.0)
    } else if gb >= 10.0 {
        format!("{gb:.0} GB")
    } else {
        format!("{gb:.1} GB")
    }
}

fn drive_tile(s: &State, p: Paint, i: usize, d: &Drive) -> El {
    let on = s.drive.selected.as_deref() == Some(d.mount.as_str());
    let busy = s.drive.running.is_some();
    let mount = d.mount.clone();
    col()
        .key(format!("drive-{i}"))
        .role("button")
        .label(d.name.clone())
        .focusable(true)
        .cursor(if busy {
            Cursor::Default
        } else {
            Cursor::Pointer
        })
        .help(if d.internal {
            "The storage built into this Mac.".to_string()
        } else {
            format!("An external drive, connected by {}.", d.connection())
        })
        .w(250.0)
        .px(14.0)
        .py(12.0)
        .gap(2.0)
        .radius(12.0)
        .fill(p.card)
        .border(1.0, if on { p.accent } else { p.line })
        .on_press(move |s: &mut State| {
            // Switching drives mid-test would hide the test that is running.
            if s.drive.running.is_none() {
                s.drive.selected = Some(mount.clone());
                s.drive.error = None;
            }
        })
        .child(t(d.name.clone(), 13.5, 650.0, p.ink).truncate())
        .child(t(d.connection(), 12.5, 500.0, p.sub).truncate())
        .child(t(
            format!("{} free of {}", size(d.free), size(d.total)),
            12.0,
            450.0,
            p.faint,
        ))
}

/// One of the two big numbers.
fn big(p: Paint, label: &str, value: Option<f64>, active: bool) -> El {
    col().grow(1.0).min_w(0.0).child(eyebrow(p, label)).child(
        row()
            .gap(8.0)
            .items_end()
            .child(
                text(value.map_or("—".to_string(), speed))
                    .font(TextStyle {
                        tracking: -1.0,
                        ..TextStyle::new(52.0, 750.0)
                    })
                    .color(match (value, active) {
                        (None, _) => p.faint,
                        (_, true) => p.accent,
                        _ => p.ink,
                    }),
            )
            .child(t("MB/s", 14.0, 600.0, p.sub).pb(12.0)),
    )
}

/// One of the two smaller numbers.
fn small(p: Paint, label: &str, value: Option<f64>, per_second: Option<f64>, active: bool) -> El {
    col()
        .grow(1.0)
        .min_w(0.0)
        .gap(1.0)
        .child(eyebrow(p, label))
        .child(t(
            value.map_or("—".to_string(), |v| format!("{} MB/s", speed(v))),
            17.0,
            700.0,
            match (value, active) {
                (None, _) => p.faint,
                (_, true) => p.accent,
                _ => p.ink,
            },
        ))
        .child(t(
            per_second.map_or(String::new(), |n| {
                format!("{} a second", speed(n.max(100.0)))
            }),
            12.0,
            450.0,
            p.sub,
        ))
}

fn group_title(p: Paint, title: &str, term: &'static Term) -> El {
    row()
        .gap(7.0)
        .items_center()
        .child(t(title, 13.5, 650.0, p.ink))
        .child(hint(p, term))
}

fn result_card(s: &State, p: Paint, d: &Drive) -> El {
    let running = s.drive.running.as_deref() == Some(d.mount.as_str());
    let result: Option<&SpeedResult> = s.drive.results.get(&d.mount);
    let phase = s.drive.progress.map(|pr| pr.phase).filter(|_| running);
    // While a test runs, show its numbers as they come in; otherwise the last result.
    let value = |ph: Phase| -> Option<f64> {
        if running {
            s.drive.live[PHASES.iter().position(|x| *x == ph).unwrap_or(0)]
        } else {
            result.map(|r| match ph {
                Phase::Write => r.write,
                Phase::Read => r.read,
                Phase::SmallRead => r.small_read,
                Phase::SmallWrite => r.small_write,
            })
        }
    };
    let action = if running {
        button(p, "speed-cancel", "Cancel", false, true, |s| {
            s.cancel_speed_test()
        })
    } else {
        button(
            p,
            "speed-start",
            if result.is_some() {
                "Test again"
            } else {
                "Start test"
            },
            true,
            !s.demo && s.drive.running.is_none(),
            |s| s.start_speed_test(),
        )
    };
    let mut c = card(p).key("speed").padding(26.0).gap(20.0).child(
        row()
            .gap(16.0)
            .items_center()
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(t(d.name.clone(), 17.0, 700.0, p.ink).truncate())
                    .child(t(d.connection(), 12.5, 500.0, p.sub)),
            )
            .child(action),
    );
    if running {
        let step = phase
            .and_then(|ph| PHASES.iter().position(|x| *x == ph))
            .unwrap_or(0);
        let fraction = s
            .drive
            .progress
            .map_or(0.0, |pr| pr.fraction.clamp(0.0, 1.0));
        let overall = ((step as f32 + fraction) / PHASES.len() as f32).clamp(0.02, 1.0);
        c = c.child(
            col()
                .gap(6.0)
                .child(
                    row()
                        .h(6.0)
                        .w_full()
                        .radius(3.0)
                        .fill(p.line)
                        .child(el().h(6.0).radius(3.0).fill(p.accent).grow(overall))
                        .child(el().grow(1.0 - overall)),
                )
                .child(t(
                    format!(
                        "Step {} of {}: {}",
                        step + 1,
                        PHASES.len(),
                        phase.map_or("Getting ready", |ph| ph.doing())
                    ),
                    12.5,
                    500.0,
                    p.sub,
                )),
        );
    }
    c = c
        .child(
            col()
                .gap(8.0)
                .child(group_title(p, "Large files", &guide::LARGE_FILES))
                .child(
                    row()
                        .gap(24.0)
                        .child(big(
                            p,
                            "Read",
                            value(Phase::Read),
                            phase == Some(Phase::Read),
                        ))
                        .child(big(
                            p,
                            "Write",
                            value(Phase::Write),
                            phase == Some(Phase::Write),
                        )),
                ),
        )
        .child(
            col()
                .gap(8.0)
                .pt(16.0)
                .border_sides([1.0, 0.0, 0.0, 0.0], p.line)
                .child(group_title(p, "Small pieces", &guide::SMALL_PIECES))
                .child(
                    row()
                        .gap(24.0)
                        .child(small(
                            p,
                            "Read",
                            value(Phase::SmallRead),
                            result.filter(|_| !running).map(|r| r.small_read_iops),
                            phase == Some(Phase::SmallRead),
                        ))
                        .child(small(
                            p,
                            "Write",
                            value(Phase::SmallWrite),
                            result.filter(|_| !running).map(|r| r.small_write_iops),
                            phase == Some(Phase::SmallWrite),
                        )),
                ),
        );
    let note = if let Some(e) = s.drive.error.as_ref().filter(|_| !running) {
        Some(t(e.clone(), 13.0, 500.0, p.bad))
    } else if let Some(r) = result.filter(|_| !running) {
        Some(t(drives::verdict(d, r.read), 13.5, 450.0, p.ink))
            .filter(|_| !drives::verdict(d, r.read).is_empty())
    } else if !running {
        Some(t(
            if s.demo {
                "Example data. The test is switched off here."
            } else {
                "The test writes one temporary file of up to 1 GB, measures it, and deletes it. It takes about 15 seconds on a fast drive and longer on a slow one. Your files aren't touched."
            },
            13.0,
            450.0,
            p.sub,
        ))
    } else {
        None
    };
    c.children(note.map(|n| n.wrap()))
}

pub fn page(s: &State, p: Paint) -> El {
    let mut page = col().gap(14.0).child(page_head(
        p,
        "Drive speed",
        "How fast a drive reads and writes, measured with a real file.".into(),
        Some(link_button(
            p,
            "drives-refresh",
            "Look for drives again",
            |s| s.refresh_drives(),
        )),
    ));
    if s.drive.drives.is_empty() {
        return page.child(card(p).child(body(p, "Looking for drives…")));
    }
    let mut tiles = row().gap(10.0).flex_wrap();
    for (i, d) in s.drive.drives.iter().enumerate() {
        tiles = tiles.child(drive_tile(s, p, i, d));
    }
    page = page.child(tiles);
    if s.drive.drives.iter().all(|d| d.internal) {
        page = page.child(t(
            "To test an external drive, plug it in and wait for it to appear in Finder.",
            12.5,
            450.0,
            p.sub,
        ));
    }
    match s.drive.chosen() {
        Some(d) => page.child(result_card(s, p, d)),
        None => page,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers_are_short_and_readable() {
        assert_eq!(speed(45.67), "45.7");
        assert_eq!(speed(968.7), "969");
        assert_eq!(speed(5210.0), "5,210");
        assert_eq!(speed(15_020.0), "15,020");
        assert_eq!(size(494_384_795_648), "494 GB");
        assert_eq!(size(2_000_000_000_000), "2.0 TB");
    }
}
