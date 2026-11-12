//! The cable drawer: named cables, each with the tests run on it, saved as JSON.
use crate::guide::Rung;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// What the cable's e-marker chip claimed at the time of a test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChipClaim {
    pub speed: Option<Rung>,
    pub watts: Option<u32>,
    pub active: bool,
}

/// One cable, one port, one device: what happened.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TestRecord {
    pub at_unix: u64,
    pub port: u32,
    pub host_max: Option<Rung>,
    pub device: String,
    #[serde(default)]
    pub device_key: String,
    pub device_max: Option<Rung>,
    #[serde(default)]
    pub device_max_certain: bool,
    /// The rung the link reached; None when no data link came up.
    pub link: Option<Rung>,
    #[serde(default)]
    pub link_label: String,
    /// Rungs both ends support that the link did not reach.
    #[serde(default)]
    pub failed: Vec<Rung>,
    /// The link reached with the plug each way up, where that was tried.
    #[serde(default)]
    pub sides: [Option<Rung>; 2],
    #[serde(default)]
    pub chip: Option<ChipClaim>,
    #[serde(default)]
    pub charge_watts: Option<u32>,
    /// A charger offered more than 3 A through this cable, which it only does for a 5 A cable.
    #[serde(default)]
    pub five_amp: bool,
    #[serde(default)]
    pub video: bool,
    #[serde(default)]
    pub link_changes: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Cable {
    pub name: String,
    #[serde(default)]
    pub notes: String,
    pub tests: Vec<TestRecord>,
}

/// Everything the tests on one cable add up to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rating {
    /// Fastest link any test reached.
    pub proven: Option<Rung>,
    /// The slowest rung the cable failed to reach with ends that both support it.
    pub fails_at: Option<Rung>,
    pub watts: Option<u32>,
    /// The fastest speed the cable's chip claims, if it has one.
    pub chip_speed: Option<Rung>,
    /// The wattage the cable's chip claims, if it has one.
    pub chip_watts: Option<u32>,
    /// True when a charger confirmed the wattage; false when it is only the chip's claim.
    pub watts_proven: bool,
    pub video: bool,
    pub unstable: bool,
    /// A test linked slower with the plug one way up than the other.
    pub lopsided: bool,
}

impl Rating {
    /// What to write on a tag.
    pub fn label(&self) -> String {
        let mut parts = vec![match self.proven {
            Some(r) => match r {
                Rung::Usb2 => "USB 2".to_string(),
                r => format!(
                    "{}{}",
                    r.speed().replace(" Gb/s", "G"),
                    // A plus: passed at this speed, and never tried with anything faster.
                    if self.fails_at.is_none() && r < Rung::Tb80 {
                        "+"
                    } else {
                        ""
                    }
                ),
            },
            None => "Charge only".to_string(),
        }];
        if let Some(w) = self.watts {
            parts.push(format!("{w}W"));
        }
        if self.video {
            parts.push("video".into());
        }
        parts.join(" · ")
    }
    pub fn summary(&self) -> String {
        let data = match (self.proven, self.fails_at) {
            (Some(p), Some(f)) => format!(
                "Tops out at {}. It failed to reach {} with devices that can.",
                p.speed(),
                f.speed()
            ),
            (Some(p), None) if p >= Rung::Tb80 => {
                format!("Carries {}, the fastest there is.", p.speed())
            }
            (Some(p), None) => format!(
                "Carries at least {}. Not yet tested with anything faster.",
                p.speed()
            ),
            (None, Some(_)) => {
                "No data link came up with devices that send data. Likely charge-only or damaged."
                    .to_string()
            }
            (None, None) => "No data link seen yet.".to_string(),
        };
        let claim = match (self.chip_speed, self.proven) {
            (Some(c), p) if self.fails_at.is_none() && p.is_none_or(|p| c > p) => {
                format!(" Its chip claims {}.", c.speed())
            }
            _ => String::new(),
        };
        let power = match (self.watts, self.watts_proven, self.chip_watts) {
            (Some(_), true, Some(w)) => format!(
                " Its chip claims {w} W charging, and a charger confirmed it is a 5 A cable."
            ),
            (Some(_), true, None) => {
                " A charger confirmed it is a 5 A cable, good for 100 W or more.".to_string()
            }
            (Some(w), false, _) => format!(" Its chip claims {w} W charging."),
            (None, ..) => String::new(),
        };
        let side = if self.lopsided {
            " Slower with the plug one way up, so a wire is probably damaged."
        } else {
            ""
        };
        let wobble = if self.unstable {
            " The link changed speed during a test, so treat it as unreliable."
        } else {
            ""
        };
        format!("{data}{claim}{power}{side}{wobble}")
    }
}

impl Cable {
    pub fn rating(&self) -> Rating {
        let proven = self.tests.iter().filter_map(|t| t.link).max();
        let fails_at = self
            .tests
            .iter()
            .flat_map(|t| t.failed.iter().copied())
            .filter(|f| proven.is_none_or(|p| *f > p))
            .min();
        let claimed = self
            .tests
            .iter()
            .filter_map(|t| t.chip.as_ref()?.watts)
            .max();
        let five_amp = self.tests.iter().any(|t| t.five_amp);
        Rating {
            proven,
            fails_at,
            watts: match (claimed, five_amp) {
                (Some(w), _) => Some(w),
                (None, true) => Some(100),
                (None, false) => None,
            },
            chip_speed: self
                .tests
                .iter()
                .filter_map(|t| t.chip.as_ref()?.speed)
                .max(),
            chip_watts: claimed,
            watts_proven: five_amp,
            video: self.tests.iter().any(|t| t.video),
            unstable: self.tests.iter().any(|t| t.link_changes > 0),
            lopsided: self
                .tests
                .iter()
                .any(|t| matches!(t.sides, [Some(a), Some(b)] if a != b)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notebook {
    pub version: u32,
    #[serde(default)]
    pub cables: Vec<Cable>,
    /// Top speeds the user has set for devices, by device key.
    #[serde(default)]
    pub devices: BTreeMap<String, Rung>,
}

impl Default for Notebook {
    fn default() -> Self {
        Self {
            version: 2,
            cables: vec![],
            devices: BTreeMap::new(),
        }
    }
}

/// The first notebook format: one flat list of observations.
#[derive(Deserialize)]
struct V1 {
    observations: Vec<V1Observation>,
}
#[derive(Deserialize)]
struct V1Observation {
    cable: String,
    notes: String,
    recorded_at_unix: u64,
    device: V1Device,
}
#[derive(Deserialize)]
struct V1Device {
    name: String,
    mbps: Option<u64>,
}

impl Notebook {
    /// The fastest link ever recorded with this device, through any cable. The device can do at
    /// least this.
    pub fn best_seen(&self, device_key: &str) -> Option<Rung> {
        if device_key.is_empty() {
            return None;
        }
        self.cables
            .iter()
            .flat_map(|c| &c.tests)
            .filter(|t| t.device_key == device_key)
            .filter_map(|t| t.link)
            .max()
    }
    /// Devices from earlier tests that reached `rung` or faster.
    pub fn devices_reaching(&self, rung: Rung) -> Vec<String> {
        let mut names: Vec<String> = self
            .cables
            .iter()
            .flat_map(|c| &c.tests)
            .filter(|t| t.link.is_some_and(|l| l >= rung))
            .map(|t| t.device.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }
    /// Adds a test to the cable with this name, creating the cable if needed.
    pub fn add_test(&mut self, name: &str, notes: &str, test: TestRecord) {
        match self
            .cables
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(name))
        {
            Some(cable) => {
                if !notes.is_empty() {
                    cable.notes = notes.into();
                }
                cable.tests.push(test);
            }
            None => self.cables.push(Cable {
                name: name.into(),
                notes: notes.into(),
                tests: vec![test],
            }),
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| format!("Can't read the saved cables; the file was left alone: {e}"))?;
        match value["version"].as_u64() {
            Some(2) => serde_json::from_value(value)
                .map_err(|e| format!("Can't read the saved cables; the file was left alone: {e}")),
            Some(1) => {
                let old: V1 = serde_json::from_value(value).map_err(|e| {
                    format!("Can't read the older saved cables; the file was left alone: {e}")
                })?;
                let mut book = Notebook::default();
                for o in old.observations {
                    book.add_test(
                        &o.cable,
                        &o.notes,
                        TestRecord {
                            at_unix: o.recorded_at_unix,
                            device: o.device.name,
                            link: o.device.mbps.and_then(Rung::from_mbps),
                            ..TestRecord::default()
                        },
                    );
                }
                Ok(book)
            }
            other => Err(format!("Unsupported saved-cables version {other:?}")),
        }
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(s) => Self::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Can't open the saved cables: {e}")),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Save path has no parent folder")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        let result = (|| {
            use std::io::Write;
            f.write_all(&bytes)
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
            fs::rename(&tmp, path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
}

pub fn default_path() -> PathBuf {
    std::env::var_os("CABLE_CHECK_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join("Library/Application Support/Cable Check/notebook.json")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test(link: Option<Rung>, failed: &[Rung]) -> TestRecord {
        TestRecord {
            device: "SSD".into(),
            device_key: "0bda:9210".into(),
            link,
            failed: failed.to_vec(),
            ..TestRecord::default()
        }
    }

    #[test]
    fn round_trip_and_same_name_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/book.json");
        let mut book = Notebook::load(&path).unwrap();
        book.add_test("Braided", "1 m", test(Some(Rung::Usb10), &[]));
        book.add_test("braided", "", test(Some(Rung::Usb2), &[]));
        book.save(&path).unwrap();
        let back = Notebook::load(&path).unwrap();
        assert_eq!(back, book);
        assert_eq!(back.cables.len(), 1);
        assert_eq!(back.cables[0].tests.len(), 2);
        assert_eq!(back.cables[0].notes, "1 m");
        assert_eq!(back.best_seen("0bda:9210"), Some(Rung::Usb10));
        assert_eq!(back.devices_reaching(Rung::Usb10), ["SSD"]);
    }

    #[test]
    fn rating_keeps_the_best_link_and_the_first_failure_above_it() {
        let mut cable = Cable::default();
        cable
            .tests
            .push(test(Some(Rung::Usb2), &[Rung::Usb5, Rung::Usb10]));
        let r = cable.rating();
        assert_eq!((r.proven, r.fails_at), (Some(Rung::Usb2), Some(Rung::Usb5)));
        assert_eq!(r.label(), "USB 2");
        // A later, better result (a reseated plug) outranks the earlier failure.
        cable.tests.push(TestRecord {
            chip: Some(ChipClaim {
                speed: Some(Rung::Usb10),
                watts: Some(240),
                active: false,
            }),
            video: true,
            ..test(Some(Rung::Usb10), &[])
        });
        let r = cable.rating();
        assert_eq!((r.proven, r.fails_at), (Some(Rung::Usb10), None));
        assert_eq!(r.label(), "10G+ · 240W · video");
        assert!(!r.watts_proven);
    }

    #[test]
    fn first_format_is_migrated() {
        let raw = r#"{"version":1,"observations":[{"cable":"Old","notes":"n","recorded_at_unix":5,
            "device":{"name":"SSD","path":"p","transport":"USB","reported_speed":"x","mbps":10000}}]}"#;
        let book = Notebook::parse(raw).unwrap();
        assert_eq!(book.version, 2);
        assert_eq!(book.cables[0].tests[0].link, Some(Rung::Usb10));
    }

    #[test]
    fn corrupt_file_is_not_silently_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.json");
        fs::write(&path, "oops").unwrap();
        assert!(Notebook::load(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "oops");
    }

    #[test]
    fn save_collision_preserves_existing_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.json");
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&tmp, "existing").unwrap();
        assert!(Notebook::default().save(&path).is_err());
        assert_eq!(fs::read_to_string(tmp).unwrap(), "existing");
        assert!(!path.exists());
    }
}
