//! Turns one port's state into a verdict about the cable: which speeds it has proven, which it
//! failed, and which this particular pair of devices can't test.
use crate::guide::{self, LADDER, Rung, Term};
use crate::ports::Port;
use crate::store::{Cable, ChipClaim, Notebook, TestRecord};

/// Where the device's top speed came from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The user picked it.
    User,
    /// The device stated it in its USB Power Delivery identity.
    Identity,
    /// A bridge chip this app recognises.
    Chip(&'static str),
    /// It linked this fast in an earlier saved test, so it can do at least this.
    Seen,
    /// Only the current link is known.
    Link,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceLimit {
    pub rung: Option<Rung>,
    /// True when `rung` is the device's real maximum; false when it is only a floor.
    pub certain: bool,
    pub source: Source,
    /// A faster speed the user picked, set aside because the device itself reports less.
    pub overruled: Option<Rung>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Proven,
    /// Not testable with this setup, but the cable's chip says it can.
    Claimed,
    Failed,
    DeviceLimit,
    MacLimit,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Caution,
    Bad,
    Quiet,
}

/// Which part of the chain set the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limiter {
    Cable,
    Device,
    Mac,
    Unknown,
    Nothing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub title: &'static str,
    pub term: &'static Term,
    pub tone: Tone,
    pub badge: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    pub connected: bool,
    pub device: String,
    pub device_key: String,
    pub device_limit: DeviceLimit,
    pub link: Option<Rung>,
    /// "10 Gb/s", or what to show when there is no data link.
    pub link_speed: String,
    pub link_name: String,
    pub host_max: Rung,
    pub rungs: Vec<(Rung, Status, String)>,
    pub tone: Tone,
    pub limiter: Limiter,
    pub headline: String,
    pub detail: String,
    /// What to plug in to test the speeds this setup couldn't.
    pub next: Option<String>,
    /// The fastest speed the cable's chip claims.
    pub claim: Option<Rung>,
    pub checks: Vec<Check>,
}

/// What has been seen on one port while the same device stays on it. Survives unplugging, so
/// a result from each side of the plug can be collected.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub device_key: String,
    pub connection: Option<u64>,
    pub since: f64,
    pub sides: [Option<Rung>; 2],
    pub tried: [bool; 2],
    pub last: Option<Rung>,
    pub changes: u32,
}

/// Links take a moment to settle: USB 2 comes up first, then the faster modes.
const SETTLE_SECONDS: f64 = 4.0;

impl Session {
    pub fn observe(&mut self, port: &Port, now: f64) {
        if !port.connected {
            return;
        }
        let (_, key) = identify(port);
        let link = link_of(port).0;
        if key != self.device_key {
            *self = Session {
                device_key: key,
                ..Session::default()
            };
        }
        if Some(port.connection_count) != self.connection {
            self.connection = Some(port.connection_count);
            self.since = now;
            self.last = link;
        }
        let settled = now - self.since >= SETTLE_SECONDS;
        if settled && link != self.last {
            self.changes += 1;
        }
        self.last = link;
        if let Some(side) = port.orientation {
            let i = side as usize - 1;
            // Keep the best while settling, then the current value.
            self.sides[i] = if settled || !self.tried[i] {
                link
            } else {
                self.sides[i].max(link)
            };
            self.tried[i] = true;
        }
    }
}

/// The device's name and a key that stays the same across cables.
pub fn identify(port: &Port) -> (String, String) {
    let usb = port.usb3.as_ref().or(port.usb2.as_ref());
    if let Some(tb) = port.thunderbolt.as_ref().filter(|t| !t.product.is_empty()) {
        return (tb.product.clone(), format!("tb:{}", tb.product));
    }
    if let Some(u) = usb {
        let name = if u.is_hub {
            match &port.display {
                Some(d) => format!("{} (monitor)", d.sink),
                None => format!("{} {}", u.vendor, u.product).trim().to_string(),
            }
        } else {
            u.product.clone()
        };
        return (name, format!("{:04x}:{:04x}", u.vid, u.pid));
    }
    if port.thunderbolt.is_some() {
        return ("Thunderbolt device".into(), "tb:unknown".into());
    }
    if let Some(d) = &port.display {
        return (d.sink.clone(), format!("dp:{}", d.sink));
    }
    if port.power.is_some() {
        return ("USB-C charger".into(), String::new());
    }
    match &port.partner {
        Some(p) => (format!("USB-C {}", p.kind), String::new()),
        None => ("Unknown device".into(), String::new()),
    }
}

/// The rung the link reached, its speed text and its protocol name.
pub fn link_of(port: &Port) -> (Option<Rung>, String, String) {
    if let Some(tb) = &port.thunderbolt {
        return match tb.mbps {
            Some(mbps) => (
                Rung::from_mbps(mbps),
                format!("{} Gb/s", mbps / 1000),
                "Thunderbolt / USB4".into(),
            ),
            None => (
                None,
                "Linked".into(),
                "Thunderbolt / USB4, speed not reported".into(),
            ),
        };
    }
    if let Some(u) = port.usb3.as_ref().or(port.usb2.as_ref()) {
        return match Rung::from_mbps(u.mbps) {
            Some(r) => (Some(r), r.speed().into(), r.name().into()),
            None => (
                None,
                format!("{} Mb/s", u.mbps),
                "USB 1.1, slower than USB 2".into(),
            ),
        };
    }
    (None, "No data".into(), String::new())
}

pub fn device_limit(port: &Port, key: &str, book: &Notebook) -> DeviceLimit {
    let link = link_of(port).0;
    let usb = port.usb3.as_ref().or(port.usb2.as_ref());
    // What the hardware says about itself: a recognised chip, or the device's own identity.
    let stated = if port.thunderbolt.is_some() {
        None
    } else if let Some((name, r)) = usb.and_then(|u| guide::known_chip(u.vid, u.pid)) {
        Some((r.max(link.unwrap_or(r)), Source::Chip(name)))
    } else {
        port.partner
            .as_ref()
            .and_then(|p| p.max)
            .filter(|r| link.is_none_or(|l| l <= *r))
            .map(|r| (r, Source::Identity))
    };
    let picked = book.devices.get(key).copied();
    match (picked, stated) {
        // The device's own word beats a faster guess: otherwise a wrong pick would put the
        // blame on a good cable.
        (Some(u), Some((r, source))) if u > r => {
            return DeviceLimit {
                rung: Some(r),
                certain: true,
                source,
                overruled: Some(u),
            };
        }
        (Some(u), _) => {
            return DeviceLimit {
                rung: Some(u),
                certain: true,
                source: Source::User,
                overruled: None,
            };
        }
        (None, Some((r, source))) => {
            return DeviceLimit {
                rung: Some(r),
                certain: true,
                source,
                overruled: None,
            };
        }
        (None, None) => {}
    }
    let (rung, source) = match (book.best_seen(key), link) {
        (Some(seen), l) if l.is_none_or(|l| seen > l) => (Some(seen), Source::Seen),
        (_, Some(l)) => (Some(l), Source::Link),
        _ => (None, Source::Unknown),
    };
    DeviceLimit {
        rung,
        certain: false,
        source,
        overruled: None,
    }
}

/// What a Mac can get out of a device of this top speed: Macs run 20 Gb/s USB gear at 10.
fn on_a_mac(r: Rung) -> Rung {
    if r == Rung::Usb20 { Rung::Usb10 } else { r }
}

/// `cable` is the saved cable being tested, when the user has named it: what it did in earlier
/// tests counts as evidence here.
pub fn analyse(port: &Port, book: &Notebook, session: &Session, cable: Option<&Cable>) -> Analysis {
    let (device, device_key) = identify(port);
    let limit = device_limit(port, &device_key, book);
    let (link, link_speed, link_name) = link_of(port);
    let host = port.host_max;
    let dev = limit.rung.map(on_a_mac);
    let tb_unknown = port.thunderbolt.as_ref().is_some_and(|t| t.mbps.is_none());
    let has_data = port.usb2.is_some() || port.usb3.is_some() || port.thunderbolt.is_some();

    let rungs: Vec<(Rung, Status, String)> = LADDER
        .iter()
        .map(|&r| {
            let (status, why) = if r == Rung::Usb20 {
                (
                    Status::MacLimit,
                    "Macs don't have 20 Gb/s USB, so no Mac can test this.".to_string(),
                )
            } else if r > host {
                (
                    Status::MacLimit,
                    format!("This port tops out at {}.", host.speed()),
                )
            } else if link.is_some_and(|l| l >= r) {
                (
                    Status::Proven,
                    if link == Some(r) {
                        "The link is running at this speed right now.".to_string()
                    } else {
                        "Covered: the link is running faster than this.".to_string()
                    },
                )
            } else if !port.connected || tb_unknown {
                (Status::Unknown, "Not tested yet.".to_string())
            } else if dev.is_some_and(|d| d >= r) {
                (
                    Status::Failed,
                    format!(
                        "The Mac and {} can both do this, but the link didn't reach it.",
                        short(&device)
                    ),
                )
            } else if limit.certain {
                (
                    Status::DeviceLimit,
                    format!(
                        "{} tops out at {}, so it can't test this.",
                        short(&device),
                        limit.rung.map_or("a lower speed", |r| r.speed())
                    ),
                )
            } else {
                (
                    Status::Unknown,
                    if has_data {
                        format!("Can't tell: {}'s top speed isn't known.", short(&device))
                    } else {
                        "No data link to judge by.".to_string()
                    },
                )
            };
            (r, status, why)
        })
        .collect();

    let claim = port.emarker.as_ref().and_then(|e| e.speed);
    let rungs: Vec<(Rung, Status, String)> = rungs
        .into_iter()
        .map(|(r, status, why)| {
            let untestable = matches!(
                status,
                Status::DeviceLimit | Status::MacLimit | Status::Unknown
            );
            if port.connected && untestable && claim.is_some_and(|c| c >= r) {
                (
                    r,
                    Status::Claimed,
                    format!("The cable's chip claims this, but it couldn't be tested here. {why}"),
                )
            } else {
                (r, status, why)
            }
        })
        .collect();
    // A speed this cable already reached with another device is not the cable's failure.
    let mut cleared: Option<(Rung, String)> = None;
    let rungs: Vec<(Rung, Status, String)> = rungs
        .into_iter()
        .map(|(r, status, why)| {
            let earlier = cable
                .filter(|_| status == Status::Failed)
                .and_then(|c| c.tests.iter().find(|t| t.link.is_some_and(|l| l >= r)));
            match earlier {
                Some(t) => {
                    cleared = Some((r, t.device.clone()));
                    (
                        r,
                        Status::Proven,
                        format!(
                            "This cable reached this speed in an earlier test with {}.",
                            t.device
                        ),
                    )
                }
                None => (r, status, why),
            }
        })
        .collect();
    let top_failed = rungs
        .iter()
        .filter(|(_, s, _)| *s == Status::Failed)
        .map(|(r, ..)| *r)
        .next_back();
    let name = short(&device);

    let (tone, limiter, headline, detail) = if !port.connected {
        (
            Tone::Quiet,
            Limiter::Nothing,
            "Nothing plugged in".to_string(),
            "Plug a device into this port with the cable you want to test.".to_string(),
        )
    } else if !has_data {
        if port.power.is_some() {
            (
                Tone::Quiet,
                Limiter::Nothing,
                "Charging only".to_string(),
                "There is no data on this port to grade. The charging result is below.".to_string(),
            )
        } else if port.display.is_some() {
            (
                Tone::Good,
                Limiter::Nothing,
                "Video works".to_string(),
                "There is no USB data on this port to grade.".to_string(),
            )
        } else {
            (
                Tone::Bad,
                Limiter::Unknown,
                "No data link".to_string(),
                "Either the cable is charge-only or the device isn't sending data. Unlock the phone, tap Trust, and look again.".to_string(),
            )
        }
    } else if tb_unknown {
        (
            Tone::Good,
            Limiter::Unknown,
            "Thunderbolt link is up".to_string(),
            "macOS didn't report its speed.".to_string(),
        )
    } else if let (None, Some((r, other)), Some(l)) = (top_failed, &cleared, link) {
        (
            Tone::Caution,
            Limiter::Device,
            "The device is the limit".to_string(),
            format!(
                "This cable reached {} with {other} before, so it is {name} holding this link at {}.",
                r.speed(),
                l.speed()
            ),
        )
    } else if let (Some(f), Some(l)) = (top_failed, link) {
        (
            Tone::Bad,
            Limiter::Cable,
            "The cable is the limit".to_string(),
            format!(
                "It links at {} and both ends can do {}. Push the plugs in, flip the one at the Mac, and look again to be sure.",
                l.speed(),
                f.speed()
            ),
        )
    } else if let Some(l) = link {
        if l >= host {
            (
                Tone::Good,
                Limiter::Nothing,
                "Passed at full speed".to_string(),
                "The fastest link this Mac can make.".to_string(),
            )
        } else if limit.certain {
            let more = match claim {
                Some(Rung::Usb20) if l == Rung::Usb10 => {
                    "The cable's chip claims 20 Gb/s, which no Mac can test.".to_string()
                }
                Some(c) if c > l => format!(
                    "The cable's chip claims {}, which this test can't confirm.",
                    c.speed()
                ),
                _ => "The cable may do more.".to_string(),
            };
            (
                Tone::Good,
                Limiter::Device,
                "Cable passed".to_string(),
                format!("That is as fast as {name} goes. {more}"),
            )
        } else {
            (
                Tone::Caution,
                Limiter::Unknown,
                "Can't tell yet".to_string(),
                format!(
                    "Can't tell whether the cable or {name} set this speed. Set the device's top speed to find out."
                ),
            )
        }
    } else {
        (
            Tone::Bad,
            Limiter::Unknown,
            "Slower than USB 2".to_string(),
            "Normal for a keyboard or mouse. A bad sign for anything else.".to_string(),
        )
    };

    // The next speed up that a faster device could test on this Mac.
    let next = (port.connected && limiter != Limiter::Cable && has_data && !tb_unknown)
        .then(|| {
            rungs.iter().find(|(r, status, _)| {
                *r != Rung::Usb20
                    && *r <= host
                    && link.is_none_or(|l| *r > l)
                    && !matches!(status, Status::Proven | Status::Failed)
            })
        })
        .flatten()
        .map(|(r, ..)| {
            let own: Vec<String> = book
                .devices_reaching(*r)
                .into_iter()
                .filter(|n| *n != device)
                .collect();
            match own.first() {
                Some(known) => format!("To test {}, use {known} with this cable.", r.speed()),
                None => format!("To test {}, plug in {}.", r.speed(), r.testers()),
            }
        });

    let checks = vec![
        chip_check(port, link, top_failed),
        power_check(port),
        video_check(port, link, has_data),
        sides_check(port, session),
        stability_check(port, session),
    ];

    Analysis {
        connected: port.connected,
        device,
        device_key,
        device_limit: limit,
        link,
        link_speed,
        link_name,
        host_max: host,
        rungs,
        tone,
        limiter,
        headline,
        detail,
        next,
        claim,
        checks,
    }
}

/// A device name short enough to sit inside a sentence.
fn short(name: &str) -> String {
    let name = name.split(" (").next().unwrap_or(name);
    if name.chars().count() > 28 {
        format!("{}…", name.chars().take(27).collect::<String>())
    } else {
        name.to_string()
    }
}

fn chip_check(port: &Port, link: Option<Rung>, failed: Option<Rung>) -> Check {
    let base = Check {
        title: "Cable's ID chip",
        term: &guide::EMARKER,
        tone: Tone::Quiet,
        badge: "Not read".into(),
        text: String::new(),
    };
    if !port.connected {
        return Check {
            text: "Read when a cable is plugged in.".into(),
            ..base
        };
    }
    let Some(e) = &port.emarker else {
        return Check {
            badge: "None found".into(),
            text: "macOS reported no e-marker chip for this cable. That is normal for USB 2 and basic USB 3 cables. Without one a cable is limited to 60 W charging, and it can't run USB4 or Thunderbolt at 40 Gb/s.".into(),
            ..base
        };
    };
    let mut claim = vec![if e.active || port.active_cable {
        "active cable".to_string()
    } else {
        "passive cable".to_string()
    }];
    if let Some(s) = e.speed {
        claim.push(if s == Rung::Usb20 {
            "up to 20 Gb/s (10 Gb/s on a Mac)".to_string()
        } else {
            format!("up to {}", s.speed())
        });
    }
    if let (Some(a), Some(w)) = (e.amps, e.watts()) {
        claim.push(format!("{a} A, so {w} W charging"));
    }
    if let Some(m) = e.metres {
        claim.push(format!("about {m} m long"));
    }
    let mut text = format!("The chip says: {}.", claim.join(", "));
    let mut tone = Tone::Good;
    let mut badge = e
        .speed
        .map_or("Found".to_string(), |s| format!("Claims {}", s.speed()));
    match (e.speed, link, failed) {
        (Some(c), Some(l), Some(f)) if f <= on_a_mac(c) => {
            tone = Tone::Bad;
            badge = "Claim not met".into();
            text.push_str(&format!(
                " The link only reached {}, with devices that can do {}. The cable is mislabelled or damaged.",
                l.speed(),
                f.speed()
            ));
        }
        (Some(c), Some(l), _) if l > c && c != Rung::Usb20 => {
            tone = Tone::Caution;
            text.push_str(&format!(
                " The link is running at {}, faster than the chip admits to. Older Thunderbolt 3 cables do this. On a no-name cable it means the chip is badly programmed.",
                l.speed()
            ));
        }
        (Some(c), Some(l), _) if l == on_a_mac(c) => text.push_str(" The link matches that."),
        _ => {}
    }
    for odd in e.oddities() {
        if tone == Tone::Good {
            tone = Tone::Caution;
        }
        text.push(' ');
        text.push_str(odd);
    }
    Check {
        tone,
        badge,
        text,
        ..base
    }
}

fn power_check(port: &Port) -> Check {
    let base = Check {
        title: "Charging",
        term: &guide::POWER_DELIVERY,
        tone: Tone::Quiet,
        badge: "Not tested".into(),
        text: "To test charging, plug a USB-C charger into the Mac with this cable. Use one above 60 W if you have it: that is the only way to tell a 5 A cable from a 3 A one.".into(),
    };
    let Some(p) = &port.power else {
        return base;
    };
    let now = format!(
        "The Mac is taking {} W ({} V at {} A) through this cable.",
        p.watts,
        trim(p.volts),
        trim(p.amps)
    );
    if p.max_offer_ma > 3000 {
        Check {
            tone: Tone::Good,
            badge: format!("{} W, 5 A cable", p.watts),
            text: format!(
                "{now} The charger is offering more than 3 A, which it only does when the cable is marked for 5 A. So this cable is good for 100 W or more."
            ),
            ..base
        }
    } else {
        Check {
            tone: Tone::Good,
            badge: format!("{} W", p.watts),
            text: format!(
                "{now} The charger offers 3 A at most here. If the charger is rated above 60 W, the cable is holding it back and is a 3 A (60 W) cable. If the charger is 60 W or smaller, this test can't tell."
            ),
            ..base
        }
    }
}

fn trim(n: f32) -> String {
    let s = format!("{n:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn video_check(port: &Port, link: Option<Rung>, has_data: bool) -> Check {
    let base = Check {
        title: "Video",
        term: &guide::ALT_MODE,
        tone: Tone::Quiet,
        badge: "Not tested".into(),
        text: "To test video, connect a USB-C monitor with this cable.".into(),
    };
    if let Some(d) = &port.display {
        return Check {
            tone: Tone::Good,
            badge: "Works".into(),
            text: format!(
                "Video is reaching {} over {} lanes at {}{}.",
                d.sink,
                d.lanes,
                if d.rate.is_empty() {
                    "an unreported rate"
                } else {
                    &d.rate
                },
                if d.tunneled {
                    ", carried inside Thunderbolt"
                } else {
                    ""
                }
            ),
            ..base
        };
    }
    if has_data && link == Some(Rung::Usb2) && port.thunderbolt.is_none() {
        return Check {
            badge: "Unlikely".into(),
            text: "The link is USB 2 only. If that is the cable's limit, it can't carry video: video uses the same high-speed wires as USB 3.".into(),
            ..base
        };
    }
    base
}

fn sides_check(port: &Port, session: &Session) -> Check {
    let base = Check {
        title: "Both ways up",
        term: &guide::ORIENTATION,
        tone: Tone::Quiet,
        badge: "Not tested".into(),
        text: "Plug in to start. Then flip the plug at the Mac to test the other side.".into(),
    };
    if port.thunderbolt.is_some() {
        return Check {
            tone: Tone::Good,
            badge: "Covered".into(),
            text: "Thunderbolt and USB4 use the wires on both sides of the plug at once, so one test covers both.".into(),
            ..base
        };
    }
    let name = |r: Option<Rung>| r.map_or("no data", |r| r.speed());
    match (session.tried, session.sides) {
        ([true, true], [a, b]) if a == b => Check {
            tone: Tone::Good,
            badge: "Same both ways".into(),
            text: format!("Both sides of the plug link at {}.", name(a)),
            ..base
        },
        ([true, true], [a, b]) => Check {
            tone: Tone::Bad,
            badge: "Sides differ".into(),
            text: format!(
                "One way up links at {}, the other at {}. A wire on one side is damaged. Mark the good side or retire the cable.",
                name(a),
                name(b)
            ),
            ..base
        },
        ([a, b], sides) if a || b => {
            let i = if a { 0 } else { 1 };
            Check {
                tone: Tone::Caution,
                badge: "One side tested".into(),
                text: format!(
                    "This side links at {}. Pull the plug out of the Mac, turn it over, and plug it back in to test the other side.",
                    name(sides[i])
                ),
                ..base
            }
        }
        _ if port.connected => Check {
            text: "macOS didn't say which way up the plug is, so the sides can't be told apart. Flip it by hand and compare the speed.".into(),
            ..base
        },
        _ => base,
    }
}

fn stability_check(port: &Port, session: &Session) -> Check {
    let base = Check {
        title: "Stability",
        term: &guide::STABILITY,
        tone: Tone::Quiet,
        badge: "Watching".into(),
        text: "Watching the link once a second. Leave it a minute and wiggle the plugs gently."
            .into(),
    };
    if !port.connected {
        return Check {
            badge: "Not tested".into(),
            text: "Starts when a cable is plugged in.".into(),
            ..base
        };
    }
    if session.changes > 0 {
        return Check {
            tone: Tone::Bad,
            badge: format!(
                "{} change{}",
                session.changes,
                if session.changes == 1 { "" } else { "s" }
            ),
            text: "The link changed speed after it had settled, without being unplugged. That means a loose plug or a cable that can only just carry this speed.".into(),
            ..base
        };
    }
    Check {
        tone: Tone::Good,
        badge: "Steady".into(),
        text: "No speed changes since the link settled. Still watching once a second, so leave it a minute and wiggle the plugs gently.".into(),
        ..base
    }
}

/// A record of the current state, ready to save.
pub fn record(port: &Port, a: &Analysis, session: &Session, at_unix: u64) -> TestRecord {
    TestRecord {
        at_unix,
        port: port.number,
        host_max: Some(a.host_max),
        device: a.device.clone(),
        device_key: a.device_key.clone(),
        device_max: a.device_limit.rung,
        device_max_certain: a.device_limit.certain,
        link: a.link,
        link_label: if a.link_name.is_empty() {
            a.link_speed.clone()
        } else {
            format!("{}, {}", a.link_speed, a.link_name)
        },
        failed: a
            .rungs
            .iter()
            .filter(|(_, s, _)| *s == Status::Failed)
            .map(|(r, ..)| *r)
            .collect(),
        sides: session.sides,
        chip: port.emarker.as_ref().map(|e| ChipClaim {
            speed: e.speed,
            watts: e.watts(),
            active: e.active,
        }),
        charge_watts: port.power.as_ref().map(|p| p.watts),
        five_amp: port.power.as_ref().is_some_and(|p| p.max_offer_ma > 3000),
        video: port.display.is_some(),
        link_changes: session.changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{self, UsbLink};

    fn status(a: &Analysis, r: Rung) -> Status {
        a.rungs.iter().find(|(x, ..)| *x == r).unwrap().1
    }
    fn ssd_port() -> Port {
        ports::example().ports.remove(1)
    }

    #[test]
    fn a_slower_device_limits_the_test_not_the_cable() {
        let a = analyse(&ssd_port(), &Notebook::default(), &Session::default(), None);
        assert_eq!(a.link, Some(Rung::Usb10));
        assert_eq!(a.device_limit.rung, Some(Rung::Usb10));
        assert!(matches!(a.device_limit.source, Source::Chip(_)));
        assert_eq!(a.limiter, Limiter::Device);
        assert_eq!(status(&a, Rung::Usb5), Status::Proven);
        assert_eq!(status(&a, Rung::Usb10), Status::Proven);
        assert_eq!(status(&a, Rung::Usb20), Status::MacLimit);
        assert_eq!(status(&a, Rung::Tb40), Status::DeviceLimit);
        assert_eq!(status(&a, Rung::Tb80), Status::DeviceLimit);
        assert!(a.next.unwrap().contains("40 Gb/s"));
    }

    #[test]
    fn a_fast_device_on_a_slow_link_blames_the_cable() {
        let mut port = ssd_port();
        let ssd = port.usb3.take().unwrap();
        port.usb2 = Some(UsbLink { mbps: 480, ..ssd });
        let a = analyse(&port, &Notebook::default(), &Session::default(), None);
        assert_eq!(a.link, Some(Rung::Usb2));
        assert_eq!(a.limiter, Limiter::Cable);
        assert_eq!(a.tone, Tone::Bad);
        assert_eq!(status(&a, Rung::Usb2), Status::Proven);
        assert_eq!(status(&a, Rung::Usb5), Status::Failed);
        assert_eq!(status(&a, Rung::Usb10), Status::Failed);
        assert_eq!(status(&a, Rung::Tb40), Status::DeviceLimit);
        assert!(a.next.is_none());
        let saved = record(&port, &a, &Session::default(), 1);
        assert_eq!(saved.failed, [Rung::Usb5, Rung::Usb10]);
    }

    #[test]
    fn an_unknown_device_gives_no_blame() {
        let mut port = ssd_port();
        port.usb3.as_mut().unwrap().pid = 0x1234;
        port.usb3.as_mut().unwrap().mbps = 5_000;
        let mut book = Notebook::default();
        let a = analyse(&port, &book, &Session::default(), None);
        assert_eq!(a.limiter, Limiter::Unknown);
        assert_eq!(status(&a, Rung::Usb10), Status::Unknown);
        // Once the user says the device can do 10 Gb/s, the 5 Gb/s link is the cable's fault.
        book.devices.insert(a.device_key.clone(), Rung::Usb10);
        let a = analyse(&port, &book, &Session::default(), None);
        assert_eq!(a.limiter, Limiter::Cable);
        assert_eq!(status(&a, Rung::Usb10), Status::Failed);
    }

    #[test]
    fn a_device_seen_faster_before_is_known_to_be_capable() {
        let mut port = ssd_port();
        port.usb3.as_mut().unwrap().pid = 0x1234;
        port.usb3.as_mut().unwrap().mbps = 5_000;
        let mut book = Notebook::default();
        book.add_test(
            "Other cable",
            "",
            TestRecord {
                device_key: "0bda:1234".into(),
                link: Some(Rung::Usb10),
                ..TestRecord::default()
            },
        );
        let a = analyse(&port, &book, &Session::default(), None);
        assert_eq!(a.device_limit.source, Source::Seen);
        assert_eq!(status(&a, Rung::Usb10), Status::Failed);
        assert_eq!(status(&a, Rung::Tb40), Status::Unknown);
    }

    #[test]
    fn the_chip_claim_is_checked_against_the_link() {
        let scan = ports::example();
        let a = analyse(
            &scan.ports[0],
            &Notebook::default(),
            &Session::default(),
            None,
        );
        // The monitor's hub tops out at 5 Gb/s, so the cable's 10 Gb/s claim is untested.
        assert_eq!(a.device_limit.rung, Some(Rung::Usb5));
        assert_eq!(a.limiter, Limiter::Device);
        let chip = &a.checks[0];
        assert_eq!(chip.tone, Tone::Caution);
        assert!(chip.text.contains("240 W"));
        assert_eq!(a.checks[1].badge, "65 W, 5 A cable");
        // The cable's chip claims 20 Gb/s: the speeds the monitor can't test show as claimed.
        assert_eq!(a.claim, Some(Rung::Usb20));
        assert_eq!(status(&a, Rung::Usb5), Status::Proven);
        assert_eq!(status(&a, Rung::Usb10), Status::Claimed);
        assert_eq!(status(&a, Rung::Usb20), Status::Claimed);
        assert_eq!(status(&a, Rung::Tb40), Status::DeviceLimit);
        assert!(a.detail.contains("claims 20 Gb/s"));
        assert_eq!(a.checks[2].badge, "Works");
    }

    #[test]
    fn sessions_collect_both_sides_and_count_drops() {
        let mut port = ssd_port();
        let mut s = Session::default();
        s.observe(&port, 1.0);
        assert_eq!(s.tried, [true, false]);
        port.orientation = Some(2);
        port.connection_count += 1;
        s.observe(&port, 20.0);
        assert_eq!(s.sides, [Some(Rung::Usb10), Some(Rung::Usb10)]);
        assert_eq!(s.changes, 0);
        // A drop to USB 2 after the link settled counts.
        let ssd = port.usb3.take().unwrap();
        port.usb2 = Some(UsbLink { mbps: 480, ..ssd });
        s.observe(&port, 30.0);
        assert_eq!(s.changes, 1);
        assert_eq!(s.sides[1], Some(Rung::Usb2));
        let check = sides_check(&port, &s);
        assert_eq!(check.tone, Tone::Bad);
    }

    #[test]
    fn a_wrong_pick_does_not_blame_a_good_cable() {
        // The monitor reports 5 Gb/s itself. Picking 10 Gb/s for it must not fail the cable.
        let port = ports::example().ports.remove(0);
        let mut book = Notebook::default();
        book.devices.insert(identify(&port).1, Rung::Usb10);
        let a = analyse(&port, &book, &Session::default(), None);
        assert_eq!(a.device_limit.rung, Some(Rung::Usb5));
        assert_eq!(a.device_limit.overruled, Some(Rung::Usb10));
        assert_eq!(a.limiter, Limiter::Device);
        assert_eq!(status(&a, Rung::Usb10), Status::Claimed);
        // A slower pick than the device reports is still honoured.
        book.devices.insert(identify(&port).1, Rung::Usb2);
        let a = analyse(&port, &book, &Session::default(), None);
        assert_eq!(a.device_limit.source, Source::User);
    }

    #[test]
    fn a_cable_that_went_faster_before_is_not_blamed() {
        let mut port = ssd_port();
        port.usb3.as_mut().unwrap().pid = 0x1234;
        port.usb3.as_mut().unwrap().mbps = 5_000;
        let mut book = Notebook::default();
        book.devices.insert("0bda:1234".into(), Rung::Usb10);
        let a = analyse(&port, &book, &Session::default(), None);
        assert_eq!(a.limiter, Limiter::Cable);
        let cable = Cable {
            name: "Grey".into(),
            notes: String::new(),
            tests: vec![TestRecord {
                device: "Fast SSD".into(),
                link: Some(Rung::Usb10),
                ..TestRecord::default()
            }],
        };
        let a = analyse(&port, &book, &Session::default(), Some(&cable));
        assert_eq!(a.limiter, Limiter::Device);
        assert_eq!(status(&a, Rung::Usb10), Status::Proven);
        assert!(a.detail.contains("Fast SSD"));
        assert!(record(&port, &a, &Session::default(), 1).failed.is_empty());
    }

    #[test]
    fn nothing_connected_is_quiet() {
        let a = analyse(
            &ports::example().ports[2],
            &Notebook::default(),
            &Session::default(),
            None,
        );
        assert!(!a.connected);
        assert_eq!(a.tone, Tone::Quiet);
        assert!(a.rungs.iter().all(|(_, s, _)| *s != Status::Failed));
    }
}
