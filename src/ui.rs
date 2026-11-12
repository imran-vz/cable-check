//! The window: a sidebar with the Mac's ports, and the bench, drawer and guide pages.
use crate::guide::{self, LADDER, Rung, Term};
use crate::ports::Port;
use crate::verdict::{self, Analysis, Limiter, Source, Status, Tone};
use crate::{Open, State, Tab};
use rune::{
    Color, Cursor, Cx, El, ScrollSpec, TextEdit, TextStyle, col, el, row, text, text_input,
};

/// Colours for one appearance.
#[derive(Clone, Copy)]
pub(crate) struct Paint {
    pub bg: Color,
    pub side: Color,
    pub card: Color,
    pub sunk: Color,
    pub line: Color,
    pub ink: Color,
    pub sub: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub on_accent: Color,
    pub good: Color,
    pub good_soft: Color,
    pub warn: Color,
    pub warn_soft: Color,
    pub bad: Color,
    pub bad_soft: Color,
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap()
}

pub(crate) fn paint(dark: bool) -> Paint {
    if dark {
        Paint {
            bg: hex("#151514"),
            side: hex("#1B1A19"),
            card: hex("#201F1D"),
            sunk: hex("#191817"),
            line: hex("#33302C"),
            ink: hex("#EEEBE5"),
            sub: hex("#A8A299"),
            faint: hex("#77726A"),
            accent: hex("#8FB0FF"),
            accent_soft: hex("#1E2944"),
            on_accent: hex("#10131C"),
            good: hex("#5CCB91"),
            good_soft: hex("#15301F"),
            warn: hex("#E7AE4A"),
            warn_soft: hex("#33280F"),
            bad: hex("#F57F6E"),
            bad_soft: hex("#3B1A15"),
        }
    } else {
        Paint {
            bg: hex("#F5F3EF"),
            side: hex("#ECE9E3"),
            card: hex("#FFFFFF"),
            sunk: hex("#F8F6F2"),
            line: hex("#E1DCD3"),
            ink: hex("#1C1A17"),
            sub: hex("#645F57"),
            faint: hex("#989288"),
            accent: hex("#2A55C4"),
            accent_soft: hex("#E6ECFB"),
            on_accent: hex("#FFFFFF"),
            good: hex("#1B7F45"),
            good_soft: hex("#E2F3E8"),
            warn: hex("#9A5B00"),
            warn_soft: hex("#FAEFD8"),
            bad: hex("#B93426"),
            bad_soft: hex("#FBE7E3"),
        }
    }
}

impl Paint {
    pub(crate) fn tone(&self, tone: Tone) -> (Color, Color) {
        match tone {
            Tone::Good => (self.good, self.good_soft),
            Tone::Caution => (self.warn, self.warn_soft),
            Tone::Bad => (self.bad, self.bad_soft),
            Tone::Quiet => (self.sub, self.sunk),
        }
    }
}

pub(crate) fn t(s: impl Into<String>, size: f32, weight: f32, color: Color) -> El {
    text(s)
        .font(TextStyle {
            line_height: (size * 1.45).round(),
            ..TextStyle::new(size, weight)
        })
        .color(color)
}
/// A small all-caps label above a block.
pub(crate) fn eyebrow(p: Paint, s: &str) -> El {
    text(s.to_uppercase())
        .font(TextStyle {
            tracking: 0.6,
            ..TextStyle::new(10.5, 650.0)
        })
        .color(p.faint)
}
pub(crate) fn body(p: Paint, s: impl Into<String>) -> El {
    t(s, 13.0, 450.0, p.sub).wrap()
}
pub(crate) fn card(p: Paint) -> El {
    col()
        .padding(22.0)
        .gap(14.0)
        .radius(14.0)
        .fill(p.card)
        .border(1.0, p.line)
}
pub(crate) fn tag(s: impl Into<String>, fg: Color, bg: Color) -> El {
    el().px(9.0)
        .py(4.0)
        .radius(6.0)
        .fill(bg)
        .child(t(s, 11.5, 650.0, fg))
}
/// A "?" that explains a term on hover.
pub(crate) fn hint(p: Paint, term: &Term) -> El {
    el().size(16.0, 16.0)
        .radius(8.0)
        .border(1.0, p.line)
        .center()
        .cursor(Cursor::Pointer)
        .label(format!("What is {}?", term.name))
        .help(format!("{}\n\n{}", term.name, term.text))
        .child(t("?", 10.5, 650.0, p.faint))
}
pub(crate) fn button(
    p: Paint,
    key: impl Into<String>,
    title: &str,
    primary: bool,
    enabled: bool,
    action: impl Fn(&mut State) + 'static,
) -> El {
    let (fill, ink, edge) = if !enabled {
        (p.sunk, p.faint, p.line)
    } else if primary {
        (p.accent, p.on_accent, p.accent)
    } else {
        (p.card, p.ink, p.line)
    };
    el().key(key)
        .role("button")
        .label(title)
        .focusable(true)
        .disabled(!enabled)
        .cursor(if enabled {
            Cursor::Pointer
        } else {
            Cursor::Default
        })
        .px(16.0)
        .py(9.0)
        .radius(9.0)
        .fill(fill)
        .border(1.0, edge)
        .on_press(move |s: &mut State| {
            if enabled {
                action(s);
            }
        })
        .child(t(title, 13.0, 600.0, ink))
}
fn field(p: Paint, edit: &TextEdit, key: &str, placeholder: &str) -> El {
    text_input(edit, TextStyle::new(14.0, 450.0))
        .key(key)
        .label(placeholder)
        .single_line()
        .placeholder(placeholder, p.faint)
        .color(p.ink)
        .w_full()
        .px(12.0)
        .py(10.0)
        .fill(p.sunk)
        .border(1.0, p.line)
        .radius(9.0)
}

fn sidebar(s: &State, p: Paint) -> El {
    let nav = |key: &str, title: String, tab: Tab, help: &str| {
        let on = s.tab == tab;
        el().key(key)
            .role("button")
            .label(title.clone())
            .focusable(true)
            .cursor(Cursor::Pointer)
            .help(help)
            .w_full()
            .px(12.0)
            .py(9.0)
            .radius(8.0)
            .fill(if on { p.card } else { p.side })
            .border(1.0, if on { p.line } else { p.side })
            .on_press(move |s: &mut State| {
                s.tab = tab;
                s.confirm_delete = None;
                if tab == Tab::Drives {
                    s.refresh_drives();
                }
            })
            .child(t(
                title,
                13.5,
                if on { 650.0 } else { 500.0 },
                if on { p.ink } else { p.sub },
            ))
    };
    let mut side = col()
        .w(244.0)
        .h_full()
        .padding(18.0)
        .gap(6.0)
        .fill(p.side)
        .border_sides([0.0, 1.0, 0.0, 0.0], p.line)
        .child(
            row()
                .gap(10.0)
                .items_center()
                .pt(26.0)
                .pb(18.0)
                .px(4.0)
                .child(
                    el().size(26.0, 26.0)
                        .radius(7.0)
                        .fill(p.ink)
                        .center()
                        .child(el().size(10.0, 4.0).radius(2.0).fill(p.side)),
                )
                .child(t("Cable Check", 16.0, 700.0, p.ink)),
        )
        .child(nav(
            "nav-bench",
            "Test bench".into(),
            Tab::Bench,
            "Test the cable that is plugged in right now.",
        ))
        .child(nav(
            "nav-drawer",
            match s.book.cables.len() {
                0 => "Cable drawer".into(),
                n => format!("Cable drawer  ·  {n}"),
            },
            Tab::Drawer,
            "Every cable you have saved, with what it turned out to be.",
        ))
        .child(nav(
            "nav-drives",
            "Drive speed".into(),
            Tab::Drives,
            "Measure how fast a drive reads and writes, the Mac's own or an external one.",
        ))
        .child(nav(
            "nav-guide",
            "Speed guide".into(),
            Tab::Guide,
            "What USB 3.2 Gen 2, USB4, Thunderbolt and the rest mean.",
        ))
        .child(el().h(14.0))
        .child(
            row()
                .px(4.0)
                .pb(4.0)
                .gap(6.0)
                .items_center()
                .child(eyebrow(p, "Ports on this Mac"))
                .child(el().size(16.0, 16.0).radius(8.0).border(1.0, p.line).center()
                    .help("macOS numbers the USB-C ports but doesn't say where each one is on the case. Plug something in and watch which row lights up.\n\nThe list updates by itself about once a second.")
                    .child(t("?", 10.5, 650.0, p.faint))),
        );
    match &s.scan {
        Some(scan) if !scan.ports.is_empty() => {
            for port in &scan.ports {
                side = side.child(port_row(s, p, port));
                if let Some(group) = port_children(p, port) {
                    side = side.child(group);
                }
            }
        }
        Some(_) => {
            side = side.child(body(p, "This Mac doesn't publish per-port cable data. Cable Check needs a Mac with Apple silicon.").px(4.0));
        }
        None => {
            side = side.child(body(p, "Reading the ports…").px(4.0));
        }
    }
    let host = s.scan.as_ref().map(|c| &c.host);
    side.child(el().grow(1.0)).child(
        col()
            .gap(2.0)
            .px(4.0)
            .child(t(
                host.map_or("This Mac", |h| h.name.as_str()),
                12.5,
                600.0,
                p.ink,
            ))
            .child(t(
                host.map_or(String::new(), |h| h.chip.clone()),
                12.0,
                450.0,
                p.sub,
            ))
            .child(
                t(
                    if s.demo {
                        "Example data. Nothing is saved."
                    } else {
                        "Cable tests only read. The drive speed test writes one temporary file and deletes it."
                    },
                    11.5,
                    450.0,
                    p.faint,
                )
                .wrap()
                .pt(6.0),
            ),
    )
}

fn port_row(s: &State, p: Paint, port: &Port) -> El {
    let on = s.port == Some(port.number) && s.tab == Tab::Bench;
    let n = port.number;
    let (name, _) = verdict::identify(port);
    let (_, speed, _) = verdict::link_of(port);
    row()
        .key(format!("port-{n}"))
        .role("button")
        .label(format!("Port {n}"))
        .focusable(true)
        .cursor(Cursor::Pointer)
        .help(format!(
            "USB-C port {n}. Fastest link it can make: {} ({}).\n\nClick to put this port on the test bench.",
            port.host_max.speed(),
            port.host_max.name()
        ))
        .w_full()
        .px(12.0)
        .py(10.0)
        .gap(10.0)
        .items_center()
        .radius(9.0)
        .fill(if on { p.card } else { p.side })
        .border(1.0, if on { p.accent } else { p.side })
        .on_press(move |s: &mut State| {
            s.port = Some(n);
            s.tab = Tab::Bench;
        })
        .child(
            el().size(8.0, 8.0)
                .radius(4.0)
                .fill(if port.connected { p.good } else { p.line }),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(t(format!("Port {n}"), 13.0, 600.0, p.ink))
                .child(
                    t(
                        if port.connected { name } else { "Empty".into() },
                        12.0,
                        450.0,
                        p.sub,
                    )
                    .truncate(),
                ),
        )
        .child(t(
            if port.connected { speed } else { String::new() },
            12.0,
            650.0,
            if port.connected { p.ink } else { p.faint },
        ))
}

/// "monitor" when the port's device shows a picture, otherwise "hub".
fn owner_word(port: &Port) -> &'static str {
    if port.display.is_some() {
        "monitor"
    } else {
        "hub"
    }
}
fn behind_speed(mbps: Option<u64>) -> String {
    match mbps {
        Some(m) => match Rung::from_mbps(m) {
            Some(r) => r.speed().to_string(),
            None => format!("{m} Mb/s"),
        },
        None => String::new(),
    }
}

/// In the sidebar: the devices plugged into a monitor's or hub's own ports, indented under the
/// Mac port they come through.
fn port_children(p: Paint, port: &Port) -> Option<El> {
    let plugged: Vec<_> = port.behind.iter().filter(|b| !b.built_in).collect();
    if !port.connected || plugged.is_empty() {
        return None;
    }
    let word = owner_word(port);
    let mut group = col()
        .ml(23.0)
        .pl(12.0)
        .py(2.0)
        .mb(4.0)
        .gap(5.0)
        .border_sides([0.0, 0.0, 0.0, 2.0], p.line)
        .help(format!(
            "These are plugged into the {word}, not into the Mac. They reach the Mac through port {}.",
            port.number
        ))
        .child(t(format!("Plugged into the {word}"), 11.0, 600.0, p.faint));
    for b in plugged {
        group = group.child(
            row()
                .gap(8.0)
                .items_center()
                .child(
                    t(b.name.clone(), 12.0, 500.0, p.sub)
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(t(behind_speed(b.mbps), 11.5, 600.0, p.sub)),
        );
    }
    Some(group)
}

/// On the bench: explains that this port leads to a monitor or hub with ports of its own, and
/// lists what is built into it and what is plugged into it.
fn behind_card(s: &State, p: Paint, port: &Port, a: &Analysis) -> Option<El> {
    let is_hub = port
        .usb3
        .as_ref()
        .or(port.usb2.as_ref())
        .is_some_and(|u| u.is_hub);
    if !port.connected || (!is_hub && port.behind.is_empty()) {
        return None;
    }
    let word = owner_word(port);
    let line = |i: usize, name: String, sub: String, speed: String, extra: Option<El>| {
        let mut title = row()
            .gap(7.0)
            .items_center()
            .child(t(name, 13.0, 600.0, p.ink));
        if let Some(e) = extra {
            title = title.child(e);
        }
        row()
            .gap(12.0)
            .py(9.0)
            .items_center()
            .border_sides([if i == 0 { 0.0 } else { 1.0 }, 0.0, 0.0, 0.0], p.line)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(title)
                    .child(t(sub, 12.0, 450.0, p.sub).wrap()),
            )
            .child(t(speed, 12.5, 650.0, p.ink))
    };
    let hubs: Vec<_> = port
        .behind
        .iter()
        .filter(|b| b.built_in && b.is_hub)
        .collect();
    let hub_speed = port
        .usb3
        .as_ref()
        .or(port.usb2.as_ref())
        .map(|u| u.mbps)
        .into_iter()
        .chain(hubs.iter().filter_map(|b| b.mbps))
        .max();
    let mut names: Vec<String> = [&port.usb2, &port.usb3]
        .into_iter()
        .flatten()
        .map(|u| u.product.clone())
        .collect();
    names.dedup();
    let mut inside = col()
        .child(eyebrow(p, &format!("Built into the {word}")))
        .child(line(
            0,
            "USB hub".into(),
            if names.is_empty() {
                format!("The part that gives the {word} its USB ports.")
            } else {
                format!(
                    "The part that gives the {word} its USB ports. macOS lists it as {}.",
                    names
                        .iter()
                        .map(|n| format!("\"{n}\""))
                        .collect::<Vec<_>>()
                        .join(" and ")
                )
            },
            behind_speed(hub_speed),
            Some(hint(p, &guide::HUB)),
        ));
    for (i, b) in port
        .behind
        .iter()
        .filter(|b| b.built_in && !b.is_hub)
        .enumerate()
    {
        inside = inside.child(line(
            i + 1,
            b.name.clone(),
            format!("Part of the {word} itself, used for its own settings."),
            behind_speed(b.mbps),
            None,
        ));
    }
    let plugged: Vec<_> = port.behind.iter().filter(|b| !b.built_in).collect();
    let mut outside = col().child(eyebrow(p, &format!("Plugged into the {word}'s USB ports")));
    if plugged.is_empty() {
        // A column stretches the text to its own width, so it wraps instead of running on.
        outside = outside.child(col().w_full().min_w(0.0).py(9.0).child(body(
            p,
            format!("Nothing right now. Plug something into the {word} and it shows up here."),
        )));
    }
    for (i, b) in plugged.iter().enumerate() {
        outside = outside.child(line(
            i,
            if b.is_hub {
                "Another USB hub".into()
            } else {
                b.name.clone()
            },
            format!("Reaches the Mac through the {word}, then through this cable."),
            behind_speed(b.mbps),
            None,
        ));
    }
    let open = s.open == Some(Open::Hub);
    let count = plugged.len();
    let mut c = card(p).key("behind").py(14.0).gap(12.0).child(
        row()
            .gap(12.0)
            .items_center()
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(t(
                        format!("This port leads to a {word} with USB ports of its own"),
                        13.5,
                        650.0,
                        p.ink,
                    ))
                    .child(t(
                        match count {
                            0 => format!("Nothing is plugged into the {word}."),
                            1 => format!("1 device is plugged into the {word}, not into the Mac."),
                            n => format!(
                                "{n} devices are plugged into the {word}, not into the Mac."
                            ),
                        },
                        12.5,
                        450.0,
                        p.sub,
                    )),
            )
            .child(hint(p, &guide::HUB))
            .child(link_button(
                p,
                "behind-toggle",
                if open { "Hide" } else { "Show" },
                |s| s.toggle(Open::Hub),
            )),
    );
    if open {
        c = c
            .child(
                row()
                    .gap(28.0)
                    .items_start()
                    .child(inside.grow(1.0).min_w(0.0))
                    .child(outside.grow(1.0).min_w(0.0)),
            )
            .child(t(
                format!(
                    "Everything on the {word} shares this one cable, so it can't go faster than {}.",
                    if a.link.is_some() { a.link_speed.as_str() } else { "the hub" },
                ),
                12.5,
                450.0,
                p.sub,
            ).wrap());
    }
    Some(c)
}

/// A text-only button for secondary actions.
pub(crate) fn link_button(
    p: Paint,
    key: &str,
    title: &str,
    action: impl Fn(&mut State) + 'static,
) -> El {
    el().key(key)
        .role("button")
        .label(title)
        .focusable(true)
        .cursor(Cursor::Pointer)
        .px(8.0)
        .py(4.0)
        .radius(6.0)
        .on_press(action)
        .child(t(title, 12.5, 600.0, p.accent))
}

/// The six speeds as one bar: what the cable proved, what its chip claims, and where this
/// setup stops being able to test.
fn meter(p: Paint, a: &Analysis) -> El {
    let mut bar = row().gap(5.0).w_full();
    for (r, status, why) in &a.rungs {
        let (word, fill, edge, ink) = match status {
            _ if !a.connected => ("", p.line, p.line, p.faint),
            Status::Proven => ("Proven", p.good, p.good, p.good),
            Status::Claimed => ("Chip claims", p.good_soft, p.good, p.good),
            Status::Failed => ("Failed", p.bad, p.bad, p.bad),
            Status::DeviceLimit => ("Device limit", p.line, p.line, p.faint),
            Status::MacLimit => ("Mac limit", p.line, p.line, p.faint),
            Status::Unknown => ("Not tested", p.line, p.line, p.faint),
        };
        let lit =
            a.connected && matches!(status, Status::Proven | Status::Claimed | Status::Failed);
        bar = bar.child(
            col()
                .key(format!("rung-{}", r.mbps()))
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .help(format!(
                    "{}  ·  {}\nAlso sold as: {}\n\n{}\n\n{}",
                    r.speed(),
                    r.name(),
                    r.also_called(),
                    why,
                    r.help()
                ))
                .child(
                    el().h(10.0)
                        .w_full()
                        .radius(5.0)
                        .fill(fill)
                        .border(1.0, edge),
                )
                .child(t(r.speed(), 12.5, 650.0, if lit { p.ink } else { p.faint }).pt(4.0))
                .child(t(word, 11.5, 550.0, ink)),
        );
    }
    bar
}

/// One of the three facts under the meter.
fn fact(p: Paint, label: &str, value: String, sub: String, warn: bool, extra: Option<El>) -> El {
    let mut head = row()
        .gap(6.0)
        .h(22.0)
        .items_center()
        .child(eyebrow(p, label));
    if warn {
        head = head.child(tag("Limit", p.warn, p.warn_soft));
    }
    let mut c = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(head)
        .child(t(value, 14.0, 650.0, p.ink).truncate())
        .child(t(sub, 12.5, 450.0, p.sub).truncate());
    if let Some(e) = extra {
        c = c.child(row().child(e));
    }
    c
}

/// Lets the user say how fast the device can go, when the app can't tell or got it wrong.
fn limit_picker(s: &State, p: Paint, a: &Analysis) -> El {
    let set = s.book.devices.get(&a.device_key).copied();
    let choice = |key: String, title: &str, value: Option<Rung>, help: String| {
        let on = set == value;
        el().key(key)
            .role("button")
            .label(title)
            .focusable(true)
            .cursor(Cursor::Pointer)
            .help(help)
            .px(11.0)
            .py(6.0)
            .radius(7.0)
            .fill(if on { p.accent_soft } else { p.card })
            .border(1.0, if on { p.accent } else { p.line })
            .on_press(move |s: &mut State| s.set_device_limit(value))
            .child(t(title, 12.5, 600.0, if on { p.accent } else { p.sub }))
    };
    let mut choices = row().gap(6.0).flex_wrap().items_center().child(choice(
        "limit-auto".into(),
        "Auto",
        None,
        "Let Cable Check work it out from the device's ID, its chip, and your earlier tests."
            .into(),
    ));
    for r in LADDER {
        choices = choices.child(choice(
            format!("limit-{}", r.mbps()),
            r.speed(),
            Some(r),
            format!("{}\n\n{}", r.name(), r.typical_devices()),
        ));
    }
    col()
        .gap(8.0)
        .padding(14.0)
        .radius(10.0)
        .fill(p.sunk)
        .child(
            row()
                .gap(6.0)
                .items_center()
                .child(t("How fast can this device go?", 12.5, 600.0, p.ink))
                .child(hint(p, &guide::BOTTLENECK)),
        )
        .child(choices)
        .children(a.device_limit.overruled.map(|picked| {
            t(
                format!(
                    "This device reports {} as its own top speed, so your pick of {} isn't used. Choose Auto to clear it.",
                    a.device_limit.rung.map_or("less", |r| r.speed()),
                    picked.speed()
                ),
                12.5,
                500.0,
                p.warn,
            )
            .wrap()
        }))
}

/// The answer: link speed, verdict, the speed meter, and the three parts of the chain.
fn hero(s: &State, p: Paint, port: &Port, a: &Analysis) -> El {
    let (fg, _) = p.tone(a.tone);
    let host = s.scan.as_ref().map_or("This Mac", |c| c.host.name.as_str());
    let mut words = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(t(a.headline.clone(), 21.0, 700.0, fg))
        .child(t(a.detail.clone(), 13.5, 450.0, p.ink).wrap());
    if let Some(next) = &a.next {
        words = words.child(t(next.clone(), 12.5, 450.0, p.sub).wrap());
    }
    let limit = &a.device_limit;
    let chip = port.emarker.as_ref();
    let facts = row()
        .gap(24.0)
        .pt(16.0)
        .items_start()
        .border_sides([1.0, 0.0, 0.0, 0.0], p.line)
        .child(
            fact(
                p,
                &format!("This Mac · port {}", port.number),
                host.to_string(),
                format!("Up to {}", a.host_max.speed()),
                a.limiter == Limiter::Mac,
                None,
            )
            .help(format!(
                "The fastest link this port can make.\n\n{}",
                a.host_max.help()
            )),
        )
        .child(
            fact(
                p,
                "The cable",
                match (a.connected, chip.and_then(|e| e.speed)) {
                    (false, _) => "Not plugged in".into(),
                    (_, Some(c)) => format!("Chip claims {}", c.speed()),
                    (_, None) if chip.is_some() => "Has an ID chip".into(),
                    _ => "No ID chip found".into(),
                },
                match chip {
                    Some(e) => [
                        e.watts().map(|w| format!("{w} W")),
                        Some(if e.active { "active" } else { "passive" }.to_string()),
                        e.metres.map(|m| format!("about {m} m")),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · "),
                    None if a.connected => "Normal for basic cables".into(),
                    None => String::new(),
                },
                a.limiter == Limiter::Cable,
                a.connected.then(|| {
                    link_button(p, "chip-details", "Details", |s| s.toggle(Open::Check(0)))
                }),
            )
            .help(format!(
                "{}\n\n{}",
                guide::EMARKER.name,
                guide::EMARKER.text
            )),
        )
        .child(
            fact(
                p,
                "Other end",
                if a.connected {
                    a.device.clone()
                } else {
                    "Nothing connected".into()
                },
                match (limit.rung, limit.certain) {
                    _ if !a.connected => String::new(),
                    (Some(r), true) => format!("Up to {}", r.speed()),
                    (Some(r), false) => format!("At least {}", r.speed()),
                    (None, _) => "Top speed unknown".into(),
                },
                a.limiter == Limiter::Device,
                (a.connected && !a.device_key.is_empty()).then(|| {
                    link_button(p, "device-change", "Change speed", |s| {
                        s.toggle(Open::Device)
                    })
                }),
            )
            .help(match &limit.source {
                Source::User => "You set this device's top speed.".to_string(),
                Source::Identity => "The device reported its own top speed.".to_string(),
                Source::Chip(name) => format!("Recognised: {name}."),
                Source::Seen => {
                    "It reached this speed in an earlier saved test. It may go faster.".to_string()
                }
                Source::Link => "Only the current link is known. It may go faster.".to_string(),
                Source::Unknown => "It hasn't sent any data yet.".to_string(),
            }),
        );
    let mut c = card(p)
        .key("hero")
        .padding(26.0)
        .gap(22.0)
        .child(
            row()
                .gap(26.0)
                .items_center()
                .child(
                    col()
                        .gap(0.0)
                        .help(format!(
                            "{}\n\n{}",
                            guide::LINK_SPEED.name,
                            guide::LINK_SPEED.text
                        ))
                        .child(eyebrow(p, "Link speed"))
                        .child(
                            text(if a.connected {
                                a.link_speed.clone()
                            } else {
                                "—".into()
                            })
                            .font(TextStyle {
                                tracking: -1.0,
                                ..TextStyle::new(52.0, 750.0)
                            })
                            .color(if a.connected {
                                p.ink
                            } else {
                                p.faint
                            }),
                        )
                        .child(t(a.link_name.clone(), 12.5, 500.0, p.sub)),
                )
                .child(el().w(1.0).h(72.0).fill(p.line))
                .child(words),
        )
        .child(meter(p, a))
        .child(facts);
    if s.open == Some(Open::Device) && a.connected && !a.device_key.is_empty() {
        c = c.child(limit_picker(s, p, a));
    }
    c
}

/// The other checks as a row of tiles; pressing one shows its explanation underneath.
fn checks(s: &State, p: Paint, a: &Analysis) -> El {
    let mut tiles = row().gap(10.0);
    for (i, check) in a.checks.iter().enumerate().skip(1) {
        let (fg, _) = p.tone(check.tone);
        let on = s.open == Some(Open::Check(i));
        tiles =
            tiles.child(
                col()
                    .key(format!("check-{i}"))
                    .role("button")
                    .label(check.title)
                    .focusable(true)
                    .cursor(Cursor::Pointer)
                    .help(format!("{}\n\n{}", check.term.name, check.term.text))
                    .grow(1.0)
                    .min_w(0.0)
                    .px(14.0)
                    .py(12.0)
                    .gap(3.0)
                    .radius(12.0)
                    .fill(p.card)
                    .border(1.0, if on { p.accent } else { p.line })
                    .on_press(move |s: &mut State| s.toggle(Open::Check(i)))
                    .child(t(check.title, 12.0, 550.0, p.sub))
                    .child(
                        row()
                            .gap(7.0)
                            .items_center()
                            .child(el().size(8.0, 8.0).radius(4.0).fill(
                                if check.tone == Tone::Quiet {
                                    p.line
                                } else {
                                    fg
                                },
                            ))
                            .child(
                                t(
                                    check.badge.clone(),
                                    13.5,
                                    650.0,
                                    if check.tone == Tone::Quiet { p.sub } else { fg },
                                )
                                .truncate(),
                            ),
                    ),
            );
    }
    let mut c = col().gap(10.0).child(tiles);
    if let Some(Open::Check(i)) = s.open {
        if let Some(check) = a.checks.get(i) {
            c = c.child(
                col()
                    .key("check-detail")
                    .padding(16.0)
                    .gap(5.0)
                    .radius(12.0)
                    .fill(p.card)
                    .border(1.0, p.line)
                    .child(
                        row()
                            .gap(7.0)
                            .items_center()
                            .child(t(check.title, 13.5, 650.0, p.ink))
                            .child(hint(p, check.term)),
                    )
                    .child(t(check.text.clone(), 13.0, 450.0, p.sub).wrap()),
            );
        }
    }
    c
}

fn save_card(s: &State, p: Paint, a: &Analysis) -> El {
    let name = s.cable.text.trim();
    let existing = s
        .book
        .cables
        .iter()
        .any(|c| c.name.eq_ignore_ascii_case(name));
    let mut c = card(p).py(14.0).gap(10.0).child(
        row()
            .gap(10.0)
            .items_center()
            .child(
                el().grow(1.0).min_w(0.0).child(
                    field(
                        p,
                        &s.cable,
                        "cable-name",
                        "Name this cable to save it, e.g. grey braided 1 m",
                    )
                    .on_edit(|s: &mut State| &mut s.cable),
                ),
            )
            .child(button(
                p,
                "save",
                if existing {
                    "Add to this cable"
                } else {
                    "Save cable"
                },
                true,
                s.can_save(),
                |s| s.save(),
            )),
    );
    if !s.book.cables.is_empty() && a.connected {
        let mut names = row().gap(6.0).flex_wrap().items_center().child(t(
            "Or add this test to:",
            12.0,
            500.0,
            p.sub,
        ));
        for (i, cable) in s.book.cables.iter().enumerate().rev().take(6) {
            names = names.child(
                el().key(format!("reuse-{i}"))
                    .role("button")
                    .label(cable.name.clone())
                    .focusable(true)
                    .cursor(Cursor::Pointer)
                    .help("Add this test to that cable instead of making a new one.")
                    .px(9.0)
                    .py(4.0)
                    .radius(6.0)
                    .border(1.0, p.line)
                    .on_press(move |s: &mut State| s.retest(i))
                    .child(t(cable.name.clone(), 12.0, 550.0, p.ink)),
            );
        }
        c = c.child(names);
    }
    if let Some(e) = &s.load_error {
        c = c.child(t(format!("Saving is off: {e}"), 12.5, 500.0, p.bad).wrap());
    } else if !s.message.is_empty() {
        c = c.child(t(s.message.clone(), 12.5, 500.0, p.good).wrap());
    }
    c
}

pub(crate) fn page_head(p: Paint, title: &str, sub: String, action: Option<El>) -> El {
    let mut r = row().items_center().gap(16.0).child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(3.0)
            .child(t(title, 26.0, 700.0, p.ink))
            .child(t(sub, 13.0, 450.0, p.sub).wrap()),
    );
    if let Some(a) = action {
        r = r.child(a);
    }
    r
}

fn copy_button(p: Paint, key: &str, title: &str, help: &str, what: fn(&State) -> String) -> El {
    el().key(key)
        .role("button")
        .label(title)
        .focusable(true)
        .cursor(Cursor::Pointer)
        .help(help)
        .px(14.0)
        .py(8.0)
        .radius(9.0)
        .border(1.0, p.line)
        .fill(p.card)
        .on_press_tx(move |s: &mut State, tx| {
            tx.copy(what(s));
            s.message = "Copied.".into();
        })
        .child(t(title, 12.5, 600.0, p.ink))
}

fn bench(s: &State, p: Paint) -> El {
    let page = col().gap(14.0);
    let (Some(port), Some(a)) = (s.selected(), s.analysis()) else {
        let (title, text) = match (&s.scan_error, &s.scan) {
            (Some(e), _) => ("Couldn't read the ports", e.clone()),
            (None, Some(_)) => (
                "No USB-C port data on this Mac",
                "Cable Check needs a Mac with Apple silicon.".to_string(),
            ),
            (None, None) => ("Reading the ports…", "This takes a second.".to_string()),
        };
        return page.child(
            card(p)
                .child(t(title, 16.0, 650.0, p.ink))
                .child(body(p, text)),
        );
    };
    page.child(hero(s, p, port, &a))
        .child(checks(s, p, &a))
        .children(behind_card(s, p, port, &a))
        .child(save_card(s, p, &a))
        .child(row().child(el().grow(1.0)).child(copy_button(
            p,
            "copy-report",
            "Copy raw report",
            "Copies everything macOS reported about the ports as JSON. Useful if a result looks wrong and you want to ask someone.",
            State::report,
        )))
}

fn date(timestamp: u64) -> String {
    i64::try_from(timestamp)
        .ok()
        .and_then(|t| time::OffsetDateTime::from_unix_timestamp(t).ok())
        .map(|d| format!("{} {} {}", d.day(), &d.month().to_string()[..3], d.year()))
        .unwrap_or_else(|| "Unknown date".into())
}

fn drawer(s: &State, p: Paint) -> El {
    let mut page = col().gap(16.0).child(page_head(
        p,
        "Cable drawer",
        "Every cable you have saved. The tag line is short enough to write on a label.".into(),
        (!s.book.cables.is_empty()).then(|| {
            copy_button(
                p,
                "copy-json",
                "Copy as JSON",
                "Copies the whole drawer as JSON, for a backup or a spreadsheet.",
                |s| serde_json::to_string_pretty(&s.book).unwrap_or_default(),
            )
        }),
    ));
    if !s.message.is_empty() {
        page = page.child(t(s.message.clone(), 12.5, 500.0, p.good));
    }
    if s.book.cables.is_empty() {
        return page.child(
            card(p)
                .child(t("Nothing saved yet", 16.0, 650.0, p.ink))
                .child(body(p, "Test a cable on the bench, give it a name, and it lands here with a rating you can copy onto a tag."))
                .child(row().child(button(p, "start-test", "Go to the test bench", true, true, |s| s.tab = Tab::Bench))),
        );
    }
    for (i, cable) in s.book.cables.iter().enumerate().rev() {
        let rating = cable.rating();
        let tone = if rating.fails_at.is_some() || rating.unstable || rating.lopsided {
            Tone::Caution
        } else if rating.proven.is_some() {
            Tone::Good
        } else {
            Tone::Quiet
        };
        let (fg, soft) = p.tone(tone);
        let mut c = card(p).key(format!("cable-{i}")).gap(10.0).child(
            row()
                .gap(12.0)
                .items_center()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(t(cable.name.clone(), 17.0, 650.0, p.ink).wrap())
                        .child(t(
                            cable.notes.clone(),
                            12.5,
                            450.0,
                            p.sub,
                        ).wrap()),
                )
                .child(
                    el().px(12.0)
                        .py(6.0)
                        .radius(8.0)
                        .fill(soft)
                        .help("Write this on the cable's tag. Speed first, then charging watts if known, then \"video\" if a display worked through it.\n\nA plus sign means the cable passed at that speed and hasn't been tried with a faster device yet, so it may do more.")
                        .child(t(rating.label(), 14.0, 700.0, fg)),
                ),
        );
        c = c.child(t(rating.summary(), 13.0, 450.0, p.ink).wrap());
        let mut tests = col().radius(9.0).fill(p.sunk).px(12.0).py(4.0);
        for (j, test) in cable.tests.iter().enumerate() {
            tests = tests.child(
                row()
                    .gap(12.0)
                    .py(7.0)
                    .items_center()
                    .border_sides([if j == 0 { 0.0 } else { 1.0 }, 0.0, 0.0, 0.0], p.line)
                    .child(t(date(test.at_unix), 12.0, 450.0, p.faint).w(86.0))
                    .child(
                        t(test.device.clone(), 12.5, 550.0, p.ink)
                            .truncate()
                            .grow(1.0)
                            .min_w(0.0),
                    )
                    .child(t(
                        if test.link_label.is_empty() {
                            test.link.map_or("No data", |l| l.speed()).to_string()
                        } else {
                            test.link_label.clone()
                        },
                        12.5,
                        500.0,
                        p.sub,
                    ))
                    .child(match test.failed.first() {
                        Some(f) => tag(format!("Failed {}", f.speed()), p.bad, p.bad_soft),
                        None => tag("Passed", p.good, p.good_soft),
                    }),
            );
        }
        let confirming = s.confirm_delete == Some(i);
        c = c.child(tests).child(
            row()
                .gap(8.0)
                .child(button(
                    p,
                    format!("retest-{i}"),
                    "Test again with another device",
                    false,
                    true,
                    move |s| s.retest(i),
                ))
                .child(el().grow(1.0))
                .child(button(
                    p,
                    format!("delete-{i}"),
                    if confirming {
                        "Press again to delete"
                    } else {
                        "Delete"
                    },
                    false,
                    true,
                    move |s| s.delete_cable(i),
                )),
        );
        page = page.child(c);
    }
    page
}

fn guide_page(p: Paint) -> El {
    let mut table = card(p).gap(0.0).child(
        col()
            .gap(3.0)
            .pb(10.0)
            .child(t("The speeds, and every name they go by", 15.0, 650.0, p.ink))
            .child(body(p, "USB has renamed the same speeds several times, so one cable can be sold under three names. The number that matters is the Gb/s.")),
    );
    for r in LADDER {
        table = table.child(
            row()
                .gap(16.0)
                .py(12.0)
                .items_start()
                .border_sides([1.0, 0.0, 0.0, 0.0], p.line)
                .help(r.help())
                .child(t(r.speed(), 17.0, 700.0, p.ink).w(92.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(t(r.name(), 13.5, 650.0, p.ink))
                        .child(
                            t(
                                format!("Also sold as: {}", r.also_called()),
                                12.5,
                                450.0,
                                p.sub,
                            )
                            .wrap(),
                        )
                        .child(t(r.in_practice(), 12.5, 450.0, p.sub).wrap()),
                )
                .child(
                    col()
                        .w(300.0)
                        .gap(2.0)
                        .child(eyebrow(p, "Test it with"))
                        .child(t(r.testers(), 12.5, 450.0, p.sub).wrap()),
                ),
        );
    }
    let mut page = col()
        .gap(16.0)
        .child(page_head(
            p,
            "Speed guide",
            "What the names on the box mean, and why a cable test can only go as fast as the slower device.".into(),
            None,
        ))
        .child(table);
    for term in guide::GLOSSARY {
        page = page.child(
            card(p)
                .gap(6.0)
                .child(t(term.name, 15.0, 650.0, p.ink))
                .child(t(term.text, 13.0, 450.0, p.sub).wrap()),
        );
    }
    page
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let p = paint(s.dark.unwrap_or_else(|| cx.system_dark()));
    let page = match s.tab {
        Tab::Bench => bench(s, p),
        Tab::Drawer => drawer(s, p),
        Tab::Drives => crate::drive_ui::page(s, p),
        Tab::Guide => guide_page(p),
    };
    row()
        .w_full()
        .h_full()
        .fill(p.bg)
        .child(sidebar(s, p))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(page.px(30.0).pt(30.0).pb(40.0).w_full()),
        )
}
