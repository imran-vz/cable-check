//! The drives a speed test can run on: the Mac's own storage and every writable external
//! volume, with how each one is connected.
use plist::{Dictionary, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    pub name: String,
    /// Where the volume is mounted; also its identity in the app.
    pub mount: String,
    /// The folder the test file goes in.
    pub test_dir: PathBuf,
    pub internal: bool,
    /// "USB", "Thunderbolt", "Apple Fabric" and so on, as macOS names the connection.
    pub bus: String,
    pub solid_state: bool,
    pub total: u64,
    pub free: u64,
    /// The whole disk behind the volume, such as "disk4".
    pub disk: String,
    /// The USB link the drive is on, in Mb/s, when it is a USB drive.
    pub link_mbps: Option<u64>,
}

impl Drive {
    /// "Built in", "USB, 10 Gb/s link", "Thunderbolt".
    pub fn connection(&self) -> String {
        if self.internal {
            return "Built in".into();
        }
        match (self.bus.as_str(), self.link_mbps) {
            ("USB", Some(m)) if m >= 1000 => format!("USB, {} Gb/s link", m / 1000),
            ("USB", Some(m)) => format!("USB, {m} Mb/s link"),
            ("", _) => "External".into(),
            (bus, _) => bus.to_string(),
        }
    }
    /// Roughly the most a USB link of this speed can carry, in MB/s. USB spends part of the
    /// link on overhead, so the figure is a bit under an eighth of the bit rate.
    pub fn link_ceiling(&self) -> Option<f64> {
        let m = self.link_mbps?;
        Some(match m {
            m if m >= 20_000 => 2_000.0,
            m if m >= 10_000 => 1_000.0,
            m if m >= 5_000 => 460.0,
            m if m >= 480 => 42.0,
            _ => m as f64 / 10.0,
        })
    }
}

/// One sentence on what a result means for this drive, or nothing when there is no cable to
/// compare against.
pub fn verdict(drive: &Drive, read: f64) -> String {
    if drive.internal {
        return "This is the Mac's built-in storage, so no cable is involved.".into();
    }
    let Some(ceiling) = drive.link_ceiling() else {
        return String::new();
    };
    let link = drive.connection().replace("USB, ", "").replace(" link", "");
    if drive.link_mbps.is_some_and(|m| m < 5_000) {
        format!(
            "This drive is on a slow USB 2 link ({link}), which carries about {ceiling:.0} MB/s at best. Check its cable on the test bench."
        )
    } else if read >= ceiling * 0.8 {
        format!(
            "That is about all a {link} link can carry (roughly {ceiling:.0} MB/s). The connection is the limit, not the drive."
        )
    } else {
        format!(
            "The {link} link could carry about {ceiling:.0} MB/s, so the drive itself is the limit here, not the cable."
        )
    }
}

fn text<'a>(d: &'a Dictionary, key: &str) -> &'a str {
    d.get(key).and_then(Value::as_string).unwrap_or("")
}
fn uint(d: &Dictionary, key: &str) -> u64 {
    d.get(key).and_then(Value::as_unsigned_integer).unwrap_or(0)
}
fn flag(d: &Dictionary, key: &str) -> bool {
    d.get(key).and_then(Value::as_boolean).unwrap_or(false)
}

/// "disk4s2" to "disk4".
fn whole_disk(part: &str) -> String {
    let digits_end = part
        .strip_prefix("disk")
        .map(|rest| 4 + rest.chars().take_while(char::is_ascii_digit).count())
        .unwrap_or(part.len());
    part[..digits_end].to_string()
}

/// One volume from `diskutil info -plist`, or None when it can't be tested (read-only, a disk
/// image, a network share).
pub fn parse_volume(bytes: &[u8]) -> Option<Drive> {
    let value = Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    let d = value.as_dictionary()?;
    let bus = text(d, "BusProtocol");
    if !flag(d, "WritableVolume") || bus == "Disk Image" {
        return None;
    }
    let mount = text(d, "MountPoint");
    if mount.is_empty() {
        return None;
    }
    // An APFS volume sits in a container; the physical disk is the container's store.
    let disk = d
        .get("APFSPhysicalStores")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_dictionary)
        .map(|s| whole_disk(text(s, "APFSPhysicalStore")))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| text(d, "ParentWholeDisk").to_string());
    let free = match uint(d, "APFSContainerFree") {
        0 => uint(d, "FreeSpace"),
        n => n,
    };
    Some(Drive {
        name: text(d, "VolumeName").to_string(),
        mount: mount.to_string(),
        test_dir: PathBuf::from(mount),
        internal: flag(d, "Internal"),
        bus: bus.to_string(),
        solid_state: flag(d, "SolidState"),
        total: uint(d, "TotalSize"),
        free,
        disk,
        link_mbps: None,
    })
}

/// Every disk name found under a node of the device tree.
fn disks_under(d: &Dictionary, out: &mut Vec<String>) {
    let name = text(d, "BSD Name");
    if !name.is_empty() {
        out.push(whole_disk(name));
    }
    for child in d
        .get("IORegistryEntryChildren")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_dictionary)
    {
        disks_under(child, out);
    }
}

/// From `ioreg -a -r -c IOUSBHostDevice -l`: which disks sit behind which USB link speed.
pub fn usb_disk_links(bytes: &[u8]) -> Vec<(String, u64)> {
    let Ok(value) = Value::from_reader(std::io::Cursor::new(bytes)) else {
        return vec![];
    };
    let roots: Vec<&Dictionary> = match &value {
        Value::Array(a) => a.iter().filter_map(Value::as_dictionary).collect(),
        Value::Dictionary(d) => vec![d],
        _ => vec![],
    };
    let mut out = vec![];
    fn walk(d: &Dictionary, out: &mut Vec<(String, u64)>) {
        let mbps = uint(d, "UsbLinkSpeed") / 1_000_000;
        if mbps > 0 {
            let mut disks = vec![];
            disks_under(d, &mut disks);
            // A drive on a hub appears under the hub too; the entry nearest the disk is
            // visited last and wins.
            for disk in disks {
                out.retain(|(name, _)| *name != disk);
                out.push((disk, mbps));
            }
        }
        for child in d
            .get("IORegistryEntryChildren")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_dictionary)
        {
            walk(child, out);
        }
    }
    for root in roots {
        walk(root, &mut out);
    }
    out
}

fn run(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = std::process::Command::new(program)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// The Mac's own storage first, then external volumes by name.
pub fn list() -> Vec<Drive> {
    let mut drives = vec![];
    // The startup volume is sealed and read-only; the user's files live on its Data volume.
    if let Some(mut own) = run(
        "/usr/sbin/diskutil",
        &["info", "-plist", "/System/Volumes/Data"],
    )
    .and_then(|b| parse_volume(&b))
    {
        own.name = run("/usr/sbin/diskutil", &["info", "-plist", "/"])
            .and_then(|b| Value::from_reader(std::io::Cursor::new(b)).ok())
            .and_then(|v| Some(text(v.as_dictionary()?, "VolumeName").to_string()))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "This Mac".into());
        own.mount = "/".into();
        own.test_dir = PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Caches/Cable Check");
        drives.push(own);
    }
    let links = run(
        "/usr/sbin/ioreg",
        &["-a", "-r", "-c", "IOUSBHostDevice", "-l"],
    )
    .map(|b| usb_disk_links(&b))
    .unwrap_or_default();
    let mut external = vec![];
    for entry in std::fs::read_dir("/Volumes")
        .into_iter()
        .flatten()
        .flatten()
    {
        // The startup volume shows up in /Volumes as a link to "/".
        if entry.file_type().is_ok_and(|t| t.is_symlink()) {
            continue;
        }
        let path = entry.path();
        let Some(mut drive) = run(
            "/usr/sbin/diskutil",
            &["info", "-plist", &path.to_string_lossy()],
        )
        .and_then(|b| parse_volume(&b)) else {
            continue;
        };
        if drive.mount == "/" || drive.mount.starts_with("/System/") {
            continue;
        }
        drive.link_mbps = links
            .iter()
            .find(|(disk, _)| *disk == drive.disk)
            .map(|(_, m)| *m);
        external.push(drive);
    }
    external.sort_by(|a, b| a.name.cmp(&b.name));
    drives.extend(external);
    drives
}

/// Drives for the example scenes.
pub fn example() -> Vec<Drive> {
    vec![
        Drive {
            name: "Macintosh HD".into(),
            mount: "/".into(),
            test_dir: PathBuf::new(),
            internal: true,
            bus: "Apple Fabric".into(),
            solid_state: true,
            total: 494_384_795_648,
            free: 112_556_699_648,
            disk: "disk0".into(),
            link_mbps: None,
        },
        Drive {
            name: "Ugreen SSD".into(),
            mount: "/Volumes/Ugreen SSD".into(),
            test_dir: PathBuf::new(),
            internal: false,
            bus: "USB".into(),
            solid_state: true,
            total: 1_000_204_886_016,
            free: 640_000_000_000,
            disk: "disk4".into(),
            link_mbps: Some(10_000),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(body: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>{body}</dict></plist>"#
        )
        .into_bytes()
    }

    #[test]
    fn reads_an_external_apfs_volume() {
        let d = parse_volume(&plist(
            "<key>VolumeName</key><string>Fast</string><key>MountPoint</key><string>/Volumes/Fast</string>\
             <key>WritableVolume</key><true/><key>Internal</key><false/><key>BusProtocol</key><string>USB</string>\
             <key>SolidState</key><true/><key>TotalSize</key><integer>1000</integer>\
             <key>APFSContainerFree</key><integer>600</integer><key>FreeSpace</key><integer>0</integer>\
             <key>ParentWholeDisk</key><string>disk5</string>\
             <key>APFSPhysicalStores</key><array><dict><key>APFSPhysicalStore</key><string>disk4s2</string></dict></array>",
        ))
        .unwrap();
        assert_eq!(
            (d.name.as_str(), d.free, d.disk.as_str()),
            ("Fast", 600, "disk4")
        );
        assert!(!d.internal);
    }

    #[test]
    fn skips_what_cannot_be_written() {
        let read_only = "<key>MountPoint</key><string>/</string><key>WritableVolume</key><false/>";
        assert!(parse_volume(&plist(read_only)).is_none());
        let image = "<key>MountPoint</key><string>/Volumes/X</string><key>WritableVolume</key><true/>\
                     <key>BusProtocol</key><string>Disk Image</string>";
        assert!(parse_volume(&plist(image)).is_none());
        assert!(parse_volume(b"junk").is_none());
    }

    #[test]
    fn finds_the_usb_link_behind_a_disk() {
        // A hub at 5 Gb/s with a 480 Mb/s stick on it: the stick's own link is the one that counts.
        let tree = plist(
            "<key>UsbLinkSpeed</key><integer>5000000000</integer><key>IORegistryEntryChildren</key><array>\
               <dict><key>UsbLinkSpeed</key><integer>480000000</integer><key>IORegistryEntryChildren</key><array>\
                 <dict><key>BSD Name</key><string>disk6</string><key>IORegistryEntryChildren</key><array>\
                   <dict><key>BSD Name</key><string>disk6s1</string></dict></array></dict></array></dict></array>",
        );
        assert_eq!(usb_disk_links(&tree), [("disk6".to_string(), 480)]);
    }

    #[test]
    fn describes_the_connection() {
        let drives = example();
        assert_eq!(drives[0].connection(), "Built in");
        assert_eq!(drives[1].connection(), "USB, 10 Gb/s link");
        assert_eq!(drives[1].link_ceiling(), Some(1_000.0));
        assert_eq!(drives[0].link_ceiling(), None);
        assert!(verdict(&drives[0], 5000.0).contains("built-in"));
        assert!(verdict(&drives[1], 950.0).contains("connection is the limit"));
        assert!(verdict(&drives[1], 400.0).contains("drive itself is the limit"));
    }
}
