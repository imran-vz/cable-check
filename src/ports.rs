//! Read-only view of each USB-C port, from the port-controller entries macOS publishes in
//! IORegistry: which transports the port supports, what link came up, the partner's and the
//! cable's USB Power Delivery identity, and the power contract.
use crate::guide::Rung;
use plist::{Dictionary, Value};
use serde::Serialize;
use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Host {
    pub name: String,
    pub model: String,
    pub chip: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Scan {
    pub host: Host,
    pub ports: Vec<Port>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Port {
    pub number: u32,
    pub supported: Vec<String>,
    pub active: Vec<String>,
    pub connected: bool,
    /// Which way up the plug is (1 or 2), when the controller says.
    pub orientation: Option<u8>,
    pub active_cable: bool,
    pub optical_cable: bool,
    /// Counts connections since boot; a change means the plug was pulled or the link reset.
    pub connection_count: u64,
    pub host_max: Rung,
    pub usb2: Option<UsbLink>,
    pub usb3: Option<UsbLink>,
    pub thunderbolt: Option<TbLink>,
    pub display: Option<DpLink>,
    pub partner: Option<Partner>,
    pub emarker: Option<EMarker>,
    pub power: Option<PowerIn>,
    /// What sits behind this port's device when that device is a hub, monitor or dock.
    pub behind: Vec<Behind>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsbLink {
    pub product: String,
    pub vendor: String,
    pub vid: u16,
    pub pid: u16,
    pub mbps: u64,
    pub rate: String,
    pub tunneled: bool,
    pub is_hub: bool,
}

/// A USB device reached through the hub on a port, not through a port on the Mac itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Behind {
    pub name: String,
    pub mbps: Option<u64>,
    /// Soldered inside the hub or monitor, as opposed to plugged into one of its ports.
    pub built_in: bool,
    pub is_hub: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct TbLink {
    pub product: String,
    pub mbps: Option<u64>,
    pub rate: String,
    /// Scalar properties of the controller's Thunderbolt state, kept for the raw report.
    pub raw: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DpLink {
    pub sink: String,
    pub lanes: u64,
    pub rate: String,
    pub tunneled: bool,
}

/// The device's answer to USB PD "Discover Identity".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Partner {
    pub vid: u16,
    pub kind: &'static str,
    /// Fastest USB signalling the device says it supports, when its identity includes that.
    pub max: Option<Rung>,
}

/// The cable's own answer to "Discover Identity" (SOP'): what its chip claims.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EMarker {
    pub vid: u16,
    pub active: bool,
    pub speed: Option<Rung>,
    pub amps: Option<u8>,
    pub max_volts: Option<u8>,
    pub epr: bool,
    /// Rough length from the chip's signal-latency field.
    pub metres: Option<u8>,
}

impl EMarker {
    pub fn watts(&self) -> Option<u32> {
        match (self.amps?, self.epr, self.max_volts) {
            (5, true, _) | (5, _, Some(50)) => Some(240),
            (5, _, _) => Some(100),
            _ => Some(60),
        }
    }
    /// Claims that don't fit how cables are normally built.
    pub fn oddities(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.vid == 0 {
            out.push(
                "The chip has no vendor ID, which is typical of unbranded cables. Treat its claims with care.",
            );
        }
        if !self.active
            && self.speed.is_some_and(|s| s >= Rung::Usb10)
            && self.metres.is_some_and(|m| m >= 3)
        {
            out.push(
                "It claims 10 Gb/s or more as a passive cable about 3 m long. Passive cables that fast are normally 1 m or shorter.",
            );
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PowerIn {
    pub watts: u32,
    pub volts: f32,
    pub amps: f32,
    /// The largest current any offered option allows, in mA.
    pub max_offer_ma: u64,
    pub max_offer_watts: u32,
}

fn text<'a>(d: &'a Dictionary, key: &str) -> Option<&'a str> {
    d.get(key).and_then(Value::as_string)
}
fn uint(d: &Dictionary, key: &str) -> Option<u64> {
    d.get(key).and_then(Value::as_unsigned_integer)
}
fn flag(d: &Dictionary, key: &str) -> bool {
    d.get(key).and_then(Value::as_boolean).unwrap_or(false)
}
fn strings(d: &Dictionary, key: &str) -> Vec<String> {
    d.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_string)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn children(d: &Dictionary) -> impl Iterator<Item = &Dictionary> {
    d.get("IORegistryEntryChildren")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_dictionary)
}
fn class<'a>(d: &'a Dictionary) -> &'a str {
    text(d, "IOObjectClass").unwrap_or("")
}

/// "10 Gbps", "480 Mbps (High Speed)", "Up to 40 Gb/s" and the like, in Mb/s.
pub fn rate_mbps(raw: &str) -> Option<u64> {
    let s = raw.to_lowercase().replace('_', " ");
    let number = s
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|p| p.chars().any(|c| c.is_ascii_digit()))?
        .parse::<f64>()
        .ok()?;
    let factor = if s.contains("gb") {
        1000.0
    } else if s.contains("mb") {
        1.0
    } else {
        return None;
    };
    let n = number * factor;
    (n.is_finite() && n > 0.0 && n <= 1_000_000.0).then_some(n as u64)
}

/// The identity VDOs a PD component reports, as host-order words.
fn vdos(d: &Dictionary) -> Vec<u32> {
    d.get("Metadata")
        .and_then(Value::as_dictionary)
        .and_then(|m| m.get("VDOs"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_data)
                .filter(|b| b.len() == 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        })
        .unwrap_or_default()
}

fn signalling(code: u32) -> Option<Rung> {
    match code {
        0 => Some(Rung::Usb2),
        1 => Some(Rung::Usb5),
        2 => Some(Rung::Usb10),
        3 => Some(Rung::Tb40),
        4 => Some(Rung::Tb80),
        _ => None,
    }
}

pub fn decode_partner(d: &Dictionary) -> Option<Partner> {
    let v = vdos(d);
    let header = *v.first()?;
    let (kind, max) = match (header >> 27) & 7 {
        1 => ("hub", v.get(3).and_then(|u| signalling(u & 7))),
        2 => ("peripheral", v.get(3).and_then(|u| signalling(u & 7))),
        // The older "alternate mode adapter" identity: 3 means USB 2 with a billboard only.
        5 => (
            "adapter or display",
            v.get(3).and_then(|u| match u & 7 {
                0 | 3 => Some(Rung::Usb2),
                1 => Some(Rung::Usb5),
                2 => Some(Rung::Usb10),
                _ => None,
            }),
        ),
        3 => ("power device", None),
        _ => ("device", None),
    };
    Some(Partner {
        vid: (header & 0xffff) as u16,
        kind,
        max,
    })
}

pub fn decode_emarker(d: &Dictionary) -> Option<EMarker> {
    let v = vdos(d);
    let header = *v.first()?;
    let active = match (header >> 27) & 7 {
        3 => false,
        4 => true,
        _ => return None,
    };
    let cable = v.get(3).copied();
    let revision = uint(d, "Specification Revision").unwrap_or(2);
    let pd3 = revision >= 3;
    Some(EMarker {
        vid: (header & 0xffff) as u16,
        active,
        // A cable marked Gen 2 carries 10 Gb/s per lane, which is 20 Gb/s on hosts that use
        // both lanes. That is the number printed on the box.
        speed: cable
            .and_then(|c| signalling(c & 7))
            .map(|r| if r == Rung::Usb10 { Rung::Usb20 } else { r }),
        amps: cable.and_then(|c| match (c >> 5) & 3 {
            1 => Some(3),
            2 => Some(5),
            _ => None,
        }),
        max_volts: cable
            .filter(|_| pd3)
            .map(|c| [20, 30, 40, 50][((c >> 9) & 3) as usize]),
        epr: pd3 && cable.is_some_and(|c| c & (1 << 17) != 0),
        metres: cable
            .map(|c| ((c >> 13) & 0xf) as u8)
            .filter(|n| (1..=8).contains(n)),
    })
}

fn usb_link(d: &Dictionary) -> Option<UsbLink> {
    let rate = text(d, "DataRateDescription")?;
    let mbps = rate_mbps(rate)?;
    if !flag(d, "Active") {
        return None;
    }
    Some(UsbLink {
        product: text(d, "Product").unwrap_or("USB device").trim().into(),
        vendor: text(d, "Manufacturer").unwrap_or("").trim().into(),
        vid: uint(d, "Vendor ID").unwrap_or(0) as u16,
        pid: uint(d, "Product ID").unwrap_or(0) as u16,
        mbps,
        rate: rate.into(),
        tunneled: flag(d, "Tunneled"),
        is_hub: uint(d, "Device Class") == Some(9),
    })
}

fn tb_link(d: &Dictionary) -> TbLink {
    let mut raw: Vec<(String, String)> = d
        .iter()
        .filter(|(k, _)| !k.starts_with("IO") && !k.starts_with("Parent"))
        .filter_map(|(k, v)| {
            let v = match v {
                Value::String(s) => s.clone(),
                Value::Boolean(b) => b.to_string(),
                Value::Integer(i) => i.to_string(),
                _ => return None,
            };
            Some((k.clone(), v))
        })
        .collect();
    raw.sort();
    let rate = raw
        .iter()
        .filter(|(k, _)| k.contains("Rate") || k.contains("Speed"))
        .find(|(_, v)| rate_mbps(v).is_some())
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    TbLink {
        product: text(d, "Product")
            .or_else(|| text(d, "ProductName"))
            .unwrap_or("")
            .into(),
        mbps: rate_mbps(&rate),
        rate,
        raw,
    }
}

fn power_in(d: &Dictionary) -> Option<PowerIn> {
    let sources: Vec<&Dictionary> = children(d).collect();
    let (source, winning) = sources.iter().find_map(|s| {
        Some((
            *s,
            s.get("WinningPowerSourceOption")
                .and_then(Value::as_dictionary)?,
        ))
    })?;
    let mw = uint(winning, "Max Power (mW)")?;
    let mv = uint(winning, "Voltage (mV)")?;
    let ma = uint(winning, "Max Current (mA)")?;
    let options: Vec<&Dictionary> = source
        .get("PowerSourceOptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_dictionary)
        .collect();
    Some(PowerIn {
        watts: (mw as f64 / 1000.0).round() as u32,
        volts: mv as f32 / 1000.0,
        amps: ma as f32 / 1000.0,
        max_offer_ma: options
            .iter()
            .filter_map(|o| uint(o, "Max Current (mA)"))
            .max()
            .unwrap_or(ma),
        max_offer_watts: options
            .iter()
            .filter_map(|o| uint(o, "Max Power (mW)"))
            .max()
            .map_or(0, |mw| (mw as f64 / 1000.0).round() as u32),
    })
}

fn port(d: &Dictionary) -> Option<Port> {
    if text(d, "PortTypeDescription") != Some("USB-C") {
        return None;
    }
    let supported = strings(d, "TransportsSupported");
    let active = strings(d, "TransportsActive");
    let on = |name: &str| active.iter().any(|a| a == name);
    let mut p = Port {
        number: uint(d, "PortNumber")? as u32,
        host_max: if supported.iter().any(|s| s == "CIO") {
            Rung::Tb40
        } else if supported.iter().any(|s| s == "USB3") {
            Rung::Usb10
        } else {
            Rung::Usb2
        },
        connected: flag(d, "ConnectionActive"),
        orientation: uint(d, "PlugOrientation")
            .filter(|n| (1..=2).contains(n))
            .map(|n| n as u8),
        active_cable: flag(d, "ActiveCable"),
        optical_cable: flag(d, "OpticalCable"),
        connection_count: uint(d, "ConnectionCount").unwrap_or(0),
        usb2: None,
        usb3: None,
        thunderbolt: None,
        display: None,
        partner: None,
        emarker: None,
        power: None,
        behind: vec![],
        supported,
        active: active.clone(),
    };
    for child in children(d) {
        let class = class(child);
        let kind = text(child, "TransportTypeDescription").unwrap_or("");
        if class.ends_with("TransportStateCC") {
            for component in children(child) {
                match text(component, "ComponentName") {
                    Some("SOP") => p.partner = decode_partner(component),
                    Some("SOP'") => p.emarker = decode_emarker(component),
                    _ => {}
                }
            }
        } else if kind == "USB3" && on("USB3") {
            p.usb3 = usb_link(child);
        } else if kind == "USB2" && on("USB2") {
            p.usb2 = usb_link(child);
        } else if kind == "DisplayPort" && on("DisplayPort") && flag(child, "Active") {
            p.display = Some(DpLink {
                sink: text(child, "ProductName").unwrap_or("a display").into(),
                lanes: uint(child, "LaneCount").unwrap_or(0),
                rate: text(child, "LinkRateDescription").unwrap_or("").into(),
                tunneled: flag(child, "Tunneled"),
            });
        } else if kind == "CIO" && on("CIO") {
            p.thunderbolt = Some(tb_link(child));
        } else if class.ends_with("FeaturePowerIn") {
            p.power = power_in(child);
        }
    }
    if on("CIO") && p.thunderbolt.is_none() {
        p.thunderbolt = Some(TbLink::default());
    }
    if !p.connected {
        p.orientation = None;
    }
    Some(p)
}

/// Ports from `ioreg -a -l -r -c IOAccessoryManager`. Empty output means this Mac publishes no
/// port controllers.
pub fn parse_ports(bytes: &[u8]) -> Result<Vec<Port>, String> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(vec![]);
    }
    let value = Value::from_reader(std::io::Cursor::new(bytes))
        .map_err(|e| format!("Couldn't read the port report: {e}"))?;
    let roots: Vec<&Dictionary> = match &value {
        Value::Array(a) => a.iter().filter_map(Value::as_dictionary).collect(),
        Value::Dictionary(d) => vec![d],
        _ => return Err("The port report isn't a device tree".into()),
    };
    let mut ports: Vec<Port> = roots.into_iter().filter_map(port).collect();
    ports.sort_by_key(|p| p.number);
    Ok(ports)
}

/// One Thunderbolt bus from `system_profiler SPThunderboltDataType -json`.
#[derive(Debug, Clone, PartialEq)]
pub struct TbBus {
    pub receptacle: u32,
    pub mbps: Option<u64>,
    pub device: Option<(String, Option<u64>, String)>,
}

pub fn parse_thunderbolt(raw: &str) -> Vec<TbBus> {
    let Ok(root) = serde_json::from_str::<Json>(raw) else {
        return vec![];
    };
    let mut out = Vec::new();
    for bus in root["SPThunderboltDataType"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let tag = &bus["receptacle_1_tag"];
        let Some(receptacle) = tag["receptacle_id_key"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let device = bus["_items"].as_array().and_then(|a| a.first()).map(|d| {
            let rate = d
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(k, _)| k.contains("upstream"))
                .find_map(|(_, v)| v["current_speed_key"].as_str())
                .or_else(|| d["link_speed_key"].as_str())
                .unwrap_or("");
            (
                d["_name"].as_str().unwrap_or("Thunderbolt device").into(),
                rate_mbps(rate),
                rate.to_string(),
            )
        });
        out.push(TbBus {
            receptacle,
            mbps: tag["current_speed_key"].as_str().and_then(rate_mbps),
            device,
        });
    }
    out
}

/// Fold what the Thunderbolt report knows into the ports: the Mac's own top speed (the fastest
/// any of its receptacles reports) and the link rate of a connected Thunderbolt device.
pub fn apply_thunderbolt(ports: &mut [Port], buses: &[TbBus], best_idle: Option<u64>) {
    let host = buses.iter().filter_map(|b| b.mbps).chain(best_idle).max();
    for p in ports.iter_mut() {
        if !p.supported.iter().any(|s| s == "CIO") {
            continue;
        }
        if let Some(rung) = host.and_then(Rung::from_mbps) {
            p.host_max = p.host_max.max(rung);
        }
        let Some(bus) = buses.iter().find(|b| b.receptacle == p.number) else {
            continue;
        };
        if let (Some(link), Some((name, mbps, rate))) = (p.thunderbolt.as_mut(), &bus.device) {
            if link.product.is_empty() {
                link.product = name.clone();
            }
            if link.mbps.is_none() {
                link.mbps = mbps.or(bus.mbps);
                link.rate = if mbps.is_some() {
                    rate.clone()
                } else {
                    bus.mbps
                        .map(|m| format!("{} Gb/s", m / 1000))
                        .unwrap_or_default()
                };
            }
        }
    }
}

pub fn parse_host(raw: &str) -> Host {
    let root: Json = serde_json::from_str(raw).unwrap_or(Json::Null);
    let h = &root["SPHardwareDataType"][0];
    let get = |k: &str| h[k].as_str().unwrap_or("").to_string();
    Host {
        name: Some(get("machine_name"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "This Mac".into()),
        model: get("machine_model"),
        chip: Some(get("chip_type"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| get("cpu_type")),
    }
}

fn run(program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Couldn't run {program}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{program} exited with {}", out.status));
    }
    Ok(out.stdout)
}

/// Walks everything under a top-level USB device.
fn collect_behind(d: &Dictionary, out: &mut Vec<Behind>) {
    for child in children(d) {
        if class(child) == "IOUSBHostDevice" {
            out.push(Behind {
                name: text(child, "USB Product Name")
                    .unwrap_or("USB device")
                    .trim()
                    .into(),
                mbps: uint(child, "UsbLinkSpeed")
                    .map(|bps| bps / 1_000_000)
                    .filter(|m| *m > 0),
                built_in: uint(child, "USBPortType") == Some(1),
                is_hub: uint(child, "bDeviceClass") == Some(9),
            });
        }
        collect_behind(child, out);
    }
}

/// Matches the USB device tree (`ioreg -a -l -p IOUSB`) to the ports: each device directly on
/// a controller is the one a port reports, and whatever hangs off it goes in that port's
/// `behind` list.
pub fn attach_usb_tree(ports: &mut [Port], bytes: &[u8]) {
    let Ok(Value::Dictionary(root)) = Value::from_reader(std::io::Cursor::new(bytes)) else {
        return;
    };
    // (port index, is the USB 3 link) pairs already matched, so twin devices on two ports
    // each find their own.
    let mut used: Vec<(usize, bool)> = vec![];
    for controller in children(&root) {
        for top in children(controller) {
            let ids = (
                uint(top, "idVendor").unwrap_or(0) as u16,
                uint(top, "idProduct").unwrap_or(0) as u16,
            );
            let slot = ports.iter().enumerate().find_map(|(i, p)| {
                [(&p.usb3, true), (&p.usb2, false)]
                    .into_iter()
                    .find(|(link, fast)| {
                        link.as_ref().is_some_and(|l| (l.vid, l.pid) == ids)
                            && !used.contains(&(i, *fast))
                    })
                    .map(|(_, fast)| (i, fast))
            });
            if let Some((i, fast)) = slot {
                used.push((i, fast));
                collect_behind(top, &mut ports[i].behind);
            }
        }
    }
}

pub fn read_ports() -> Result<Vec<Port>, String> {
    let mut ports = parse_ports(&run(
        "/usr/sbin/ioreg",
        &["-a", "-l", "-r", "-c", "IOAccessoryManager"],
    )?)?;
    if let Ok(tree) = run("/usr/sbin/ioreg", &["-a", "-l", "-p", "IOUSB"]) {
        attach_usb_tree(&mut ports, &tree);
    }
    Ok(ports)
}
pub fn read_thunderbolt() -> Vec<TbBus> {
    run(
        "/usr/sbin/system_profiler",
        &["-json", "SPThunderboltDataType"],
    )
    .map(|b| parse_thunderbolt(&String::from_utf8_lossy(&b)))
    .unwrap_or_default()
}
pub fn read_host() -> Host {
    run(
        "/usr/sbin/system_profiler",
        &["-json", "SPHardwareDataType"],
    )
    .map(|b| parse_host(&String::from_utf8_lossy(&b)))
    .unwrap_or_else(|_| Host {
        name: "This Mac".into(),
        ..Host::default()
    })
}

/// One complete read, for `--scan-json`.
pub fn scan_once() -> Result<Scan, String> {
    let mut ports = read_ports()?;
    apply_thunderbolt(&mut ports, &read_thunderbolt(), None);
    Ok(Scan {
        host: read_host(),
        ports,
    })
}

/// Watches the ports from a background thread and sends a new [`Scan`] whenever something
/// changes. The port read takes a few milliseconds; the slower Thunderbolt report is re-read
/// only when the set of Thunderbolt links changes.
pub fn watch(tx: std::sync::mpsc::Sender<Result<Scan, String>>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let host = read_host();
        let mut buses = read_thunderbolt();
        let mut tb_ports: Vec<u32> = vec![];
        let mut best_idle: Option<u64> = None;
        let mut last: Option<Result<Scan, String>> = None;
        loop {
            let next = read_ports().map(|mut ports| {
                let now: Vec<u32> = ports
                    .iter()
                    .filter(|p| p.thunderbolt.is_some())
                    .map(|p| p.number)
                    .collect();
                if now != tb_ports {
                    // Give the Thunderbolt stack a moment to finish bringing the link up.
                    if !now.is_empty() {
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                    }
                    let fresh = read_thunderbolt();
                    // Keep the fastest idle rate seen: a busy receptacle reports its link rate.
                    best_idle = buses
                        .iter()
                        .chain(&fresh)
                        .filter_map(|b| b.mbps)
                        .chain(best_idle)
                        .max();
                    buses = fresh;
                    tb_ports = now;
                }
                apply_thunderbolt(&mut ports, &buses, best_idle);
                Scan {
                    host: host.clone(),
                    ports,
                }
            });
            if last.as_ref() != Some(&next) {
                last = Some(next.clone());
                if tx.send(next).is_err() {
                    return;
                }
                wake();
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    });
}

/// The ports captured from a real MacBook Pro, for the example scenes and tests.
pub fn example() -> Scan {
    let mut ports = parse_ports(include_bytes!("../tests/fixtures/ports.plist")).unwrap();
    // The tree has two made-up devices plugged into the monitor, to show how they are grouped.
    attach_usb_tree(
        &mut ports,
        include_bytes!("../tests/fixtures/usb-tree.plist"),
    );
    apply_thunderbolt(
        &mut ports,
        &[TbBus {
            receptacle: 1,
            mbps: Some(120_000),
            device: None,
        }],
        None,
    );
    Scan {
        host: Host {
            name: "MacBook Pro".into(),
            model: "Mac16,7".into(),
            chip: "Apple M4 Pro".into(),
        },
        ports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_usb_c_port_and_skips_magsafe() {
        let scan = example();
        assert_eq!(
            scan.ports.iter().map(|p| p.number).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(scan.ports.iter().all(|p| p.host_max == Rung::Tb80));
        assert!(!scan.ports[2].connected);
    }

    #[test]
    fn reads_the_negotiated_links() {
        let scan = example();
        let hub = scan.ports[0].usb3.as_ref().unwrap();
        assert_eq!((hub.mbps, hub.is_hub), (5_000, true));
        assert_eq!(scan.ports[0].usb2.as_ref().unwrap().mbps, 480);
        let ssd = scan.ports[1].usb3.as_ref().unwrap();
        assert_eq!(ssd.product, "Ugreen Storage Device");
        assert_eq!((ssd.vid, ssd.pid, ssd.mbps), (0x0bda, 0x9210, 10_000));
        // An idle USB 2 state ("None") is not a link.
        assert!(scan.ports[1].usb2.is_none());
        let dp = scan.ports[0].display.as_ref().unwrap();
        assert_eq!((dp.lanes, dp.sink.as_str()), (2, "BenQ GW2790QT"));
    }

    #[test]
    fn decodes_the_cable_chip_and_partner_identity() {
        let scan = example();
        let e = scan.ports[0].emarker.as_ref().unwrap();
        assert!(!e.active);
        assert_eq!(e.speed, Some(Rung::Usb20));
        assert_eq!((e.amps, e.max_volts, e.epr), (Some(5), Some(50), true));
        assert_eq!(e.watts(), Some(240));
        assert_eq!(e.metres, Some(3));
        assert_eq!(e.oddities().len(), 2);
        let partner = scan.ports[0].partner.as_ref().unwrap();
        assert_eq!(partner.max, Some(Rung::Usb5));
        assert!(scan.ports[1].emarker.is_none());
    }

    #[test]
    fn devices_behind_a_monitor_belong_to_its_port() {
        let scan = example();
        let behind = &scan.ports[0].behind;
        let plugged: Vec<&str> = behind
            .iter()
            .filter(|b| !b.built_in)
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(plugged, ["USB Keyboard", "Flash Drive"]);
        assert_eq!(behind.iter().filter(|b| b.built_in && b.is_hub).count(), 2);
        assert!(behind.iter().any(|b| b.built_in && !b.is_hub));
        // The SSD is plugged straight into the Mac: nothing sits behind it.
        assert!(scan.ports[1].behind.is_empty());
    }

    #[test]
    fn reads_the_power_contract() {
        let power = example().ports[0].power.clone().unwrap();
        assert_eq!(power.watts, 65);
        assert_eq!(power.max_offer_ma, 3250);
        assert!(example().ports[1].power.is_none());
    }

    #[test]
    fn thunderbolt_report_gives_host_speed_and_device_link() {
        let raw = r#"{"SPThunderboltDataType":[
          {"_name":"bus_0","receptacle_1_tag":{"current_speed_key":"Up to 120 Gb/s","receptacle_id_key":"1"}},
          {"_name":"bus_1","receptacle_1_tag":{"current_speed_key":"Up to 40 Gb/s","receptacle_id_key":"2"},
           "_items":[{"_name":"Fast SSD","receptacle_upstream_ambiguous_tag":{"current_speed_key":"Up to 40 Gb/s"}}]}]}"#;
        let buses = parse_thunderbolt(raw);
        assert_eq!(buses.len(), 2);
        let mut ports = example().ports;
        ports[1].thunderbolt = Some(TbLink::default());
        apply_thunderbolt(&mut ports, &buses, None);
        assert_eq!(ports[1].host_max, Rung::Tb80);
        let link = ports[1].thunderbolt.as_ref().unwrap();
        assert_eq!(
            (link.product.as_str(), link.mbps),
            ("Fast SSD", Some(40_000))
        );
    }

    #[test]
    fn rates_parse_and_junk_does_not() {
        assert_eq!(rate_mbps("480 Mbps (High Speed)"), Some(480));
        assert_eq!(rate_mbps("10 Gbps"), Some(10_000));
        assert_eq!(rate_mbps("Up to 40 Gb/s"), Some(40_000));
        assert_eq!(rate_mbps("None"), None);
        assert!(parse_ports(b"").unwrap().is_empty());
        assert!(parse_ports(b"junk").is_err());
    }
}
