mod drive_ui;
mod drives;
mod guide;
mod ports;
mod speed;
mod store;
mod ui;
mod verdict;

use guide::Rung;
use ports::{Port, Scan};
use rune::{AppSpec, TextEdit, Tx};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use store::Notebook;
use verdict::{Analysis, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Bench,
    Drawer,
    Drives,
    Guide,
}

/// The one detail panel that is open on the bench, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    Check(usize),
    Hub,
    Device,
}

enum DriveMsg {
    List(Vec<drives::Drive>),
    Progress(speed::Progress),
    Done(Result<speed::SpeedResult, String>),
}

/// The drive speed page: the drives found, the test in flight, and results so far.
#[derive(Default)]
pub struct DriveTest {
    pub drives: Vec<drives::Drive>,
    /// The mount point of the chosen drive.
    pub selected: Option<String>,
    /// The mount point of the drive being tested.
    pub running: Option<String>,
    pub progress: Option<speed::Progress>,
    /// Speeds measured so far in the running test, one per phase.
    pub live: [Option<f64>; 4],
    pub results: BTreeMap<String, speed::SpeedResult>,
    pub error: Option<String>,
    cancel: Arc<AtomicBool>,
    channel: Option<(mpsc::Sender<DriveMsg>, mpsc::Receiver<DriveMsg>)>,
}

impl DriveTest {
    pub fn chosen(&self) -> Option<&drives::Drive> {
        let mount = self.selected.as_ref()?;
        self.drives.iter().find(|d| d.mount == *mount)
    }
}

pub struct State {
    pub drive: DriveTest,
    waker: Option<rune::Waker>,
    pub open: Option<Open>,
    pub tab: Tab,
    pub scan: Option<Scan>,
    pub scan_error: Option<String>,
    /// The port number on the bench.
    pub port: Option<u32>,
    pub sessions: BTreeMap<u32, Session>,
    pub cable: TextEdit,
    pub notes: TextEdit,
    pub book: Notebook,
    pub path: PathBuf,
    pub load_error: Option<String>,
    pub message: String,
    /// The cable whose delete button was pressed once and is waiting for a second press.
    pub confirm_delete: Option<usize>,
    /// Example data: nothing is read from the Mac or written to disk.
    pub demo: bool,
    pub dark: Option<bool>,
    rx: Option<mpsc::Receiver<Result<Scan, String>>>,
    now: f64,
}

impl Default for State {
    fn default() -> Self {
        let path = store::default_path();
        // Headless snapshots and scripted checks must never touch the real drawer.
        let demo = cfg!(test) || rune::tooling::headless_run().is_some();
        let loaded = if demo {
            Ok(Notebook::default())
        } else {
            Notebook::load(&path)
        };
        let load_error = loaded.as_ref().err().cloned();
        let mut s = Self {
            drive: DriveTest::default(),
            waker: None,
            open: None,
            tab: Tab::Bench,
            scan: None,
            scan_error: None,
            port: None,
            sessions: BTreeMap::new(),
            cable: TextEdit::new(""),
            notes: TextEdit::new(""),
            book: loaded.unwrap_or_default(),
            path,
            load_error,
            message: String::new(),
            confirm_delete: None,
            demo,
            dark: None,
            rx: None,
            now: 0.0,
        };
        if demo {
            s.drive.drives = drives::example();
            s.drive.selected = Some("/".into());
            s.apply(Ok(ports::example()));
            s.port = Some(2);
        }
        s
    }
}

impl State {
    /// Takes a fresh scan: updates each port's session and moves the bench to a port that was
    /// just plugged in.
    pub fn apply(&mut self, result: Result<Scan, String>) {
        let scan = match result {
            Ok(scan) => scan,
            Err(e) => {
                self.scan_error = Some(e);
                return;
            }
        };
        self.scan_error = None;
        let plugged = scan.ports.iter().find(|p| {
            p.connected
                && self
                    .scan
                    .as_ref()
                    .and_then(|old| old.ports.iter().find(|o| o.number == p.number))
                    .is_some_and(|old| !old.connected || old.connection_count != p.connection_count)
        });
        if let Some(p) = plugged {
            self.port = Some(p.number);
        } else if self
            .port
            .is_none_or(|n| !scan.ports.iter().any(|p| p.number == n))
        {
            // Start on a port with a data device, since that is what gets graded.
            self.port = scan
                .ports
                .iter()
                .find(|p| p.connected && p.power.is_none())
                .or_else(|| scan.ports.iter().find(|p| p.connected))
                .or(scan.ports.first())
                .map(|p| p.number);
        }
        for p in &scan.ports {
            self.sessions
                .entry(p.number)
                .or_default()
                .observe(p, self.now);
        }
        self.scan = Some(scan);
    }
    fn drive_sender(&mut self) -> mpsc::Sender<DriveMsg> {
        self.drive
            .channel
            .get_or_insert_with(mpsc::channel)
            .0
            .clone()
    }
    fn wake(&self) -> impl Fn() + Send + 'static {
        let waker = self.waker.clone();
        move || {
            if let Some(w) = &waker {
                w()
            }
        }
    }
    /// Looks for drives again, off the UI thread.
    pub fn refresh_drives(&mut self) {
        if self.demo {
            return;
        }
        let tx = self.drive_sender();
        let wake = self.wake();
        std::thread::spawn(move || {
            let _ = tx.send(DriveMsg::List(drives::list()));
            wake();
        });
    }
    pub fn start_speed_test(&mut self) {
        if self.demo || self.drive.running.is_some() {
            return;
        }
        let Some(drive) = self.drive.chosen().cloned() else {
            return;
        };
        let Some(plan) = speed::Plan::for_free_space(drive.free) else {
            self.drive.error =
                Some("This drive has less than 1 GB free, which is too little to test.".into());
            return;
        };
        self.drive.error = None;
        self.drive.live = [None; 4];
        self.drive.progress = None;
        self.drive.running = Some(drive.mount.clone());
        self.drive.results.remove(&drive.mount);
        let cancel = Arc::new(AtomicBool::new(false));
        self.drive.cancel = cancel.clone();
        let tx = self.drive_sender();
        let wake = self.wake();
        std::thread::spawn(move || {
            let result = speed::run(&drive.test_dir, plan, &cancel, |p| {
                let _ = tx.send(DriveMsg::Progress(p));
                wake();
            });
            let _ = tx.send(DriveMsg::Done(result));
            wake();
        });
    }
    pub fn cancel_speed_test(&mut self) {
        self.drive.cancel.store(true, Ordering::Relaxed);
    }
    fn drive_message(&mut self, msg: DriveMsg) {
        let d = &mut self.drive;
        match msg {
            DriveMsg::List(list) => {
                if d.selected
                    .as_ref()
                    .is_none_or(|m| !list.iter().any(|x| x.mount == *m))
                {
                    d.selected = list.first().map(|x| x.mount.clone());
                }
                d.drives = list;
            }
            DriveMsg::Progress(p) => {
                let i = speed::PHASES
                    .iter()
                    .position(|x| *x == p.phase)
                    .unwrap_or(0);
                d.live[i] = Some(p.mbps);
                d.progress = Some(p);
            }
            DriveMsg::Done(result) => {
                let mount = d.running.take();
                d.progress = None;
                match (result, mount) {
                    (Ok(r), Some(m)) => {
                        d.results.insert(m, r);
                    }
                    (Err(e), _) if e == "Cancelled" => {}
                    (Err(e), _) => d.error = Some(e),
                    _ => {}
                }
            }
        }
    }
    /// Opens a detail panel, or closes it if it is already open.
    pub fn toggle(&mut self, panel: Open) {
        self.open = if self.open == Some(panel) {
            None
        } else {
            Some(panel)
        };
    }
    pub fn selected(&self) -> Option<&Port> {
        let n = self.port?;
        self.scan.as_ref()?.ports.iter().find(|p| p.number == n)
    }
    pub fn session(&self, port: u32) -> Session {
        self.sessions.get(&port).cloned().unwrap_or_default()
    }
    pub fn analysis(&self) -> Option<Analysis> {
        let p = self.selected()?;
        let name = self.cable.text.trim();
        let cable = self
            .book
            .cables
            .iter()
            .find(|c| !name.is_empty() && c.name.eq_ignore_ascii_case(name));
        Some(verdict::analyse(
            p,
            &self.book,
            &self.session(p.number),
            cable,
        ))
    }
    pub fn can_save(&self) -> bool {
        self.load_error.is_none()
            && !self.cable.text.trim().is_empty()
            && self.selected().is_some_and(|p| p.connected)
    }
    /// Writes the drawer to disk, or says why it couldn't. Demo data stays in memory.
    fn persist(&mut self, next: Notebook, done: &str) {
        let result = if self.demo {
            Ok(())
        } else {
            next.save(&self.path)
        };
        match result {
            Ok(()) => {
                self.book = next;
                self.message = done.into();
            }
            Err(e) => self.message = format!("Couldn't save: {e}"),
        }
    }
    pub fn save(&mut self) {
        if !self.can_save() {
            return;
        }
        let (Some(port), Some(a)) = (self.selected().cloned(), self.analysis()) else {
            return;
        };
        let at = if self.demo {
            1_794_500_000
        } else {
            store::now_unix()
        };
        let test = verdict::record(&port, &a, &self.session(port.number), at);
        let name = self.cable.text.trim().to_string();
        let mut next = self.book.clone();
        next.add_test(&name, self.notes.text.trim(), test);
        self.persist(
            next,
            &format!("Saved \"{name}\". Eject any drive before you unplug it, then swap in the next cable."),
        );
        if self.message.starts_with("Saved") {
            self.cable = TextEdit::new("");
            self.notes = TextEdit::new("");
        }
    }
    /// Sets or clears the user's top speed for the device on the bench.
    pub fn set_device_limit(&mut self, limit: Option<Rung>) {
        let Some(key) = self
            .analysis()
            .map(|a| a.device_key)
            .filter(|k| !k.is_empty())
        else {
            return;
        };
        let mut next = self.book.clone();
        match limit {
            Some(r) => next.devices.insert(key, r),
            None => next.devices.remove(&key),
        };
        self.persist(next, "");
    }
    pub fn delete_cable(&mut self, index: usize) {
        if self.confirm_delete != Some(index) {
            self.confirm_delete = Some(index);
            return;
        }
        self.confirm_delete = None;
        if index < self.book.cables.len() {
            let mut next = self.book.clone();
            let gone = next.cables.remove(index);
            self.persist(next, &format!("Deleted \"{}\".", gone.name));
        }
    }
    /// Puts a saved cable's name on the bench so another test is added to it.
    pub fn retest(&mut self, index: usize) {
        if let Some(c) = self.book.cables.get(index) {
            self.cable = TextEdit::new(&c.name);
            self.notes = TextEdit::new(&c.notes);
            self.tab = Tab::Bench;
        }
    }
    /// Everything on the bench as JSON, for bug reports.
    pub fn report(&self) -> String {
        serde_json::to_string_pretty(&self.scan).unwrap_or_default()
    }
}

fn setup(s: &mut State, waker: rune::Waker) {
    if s.demo {
        return;
    }
    s.waker = Some(waker.clone());
    s.refresh_drives();
    let (tx, rx) = mpsc::channel();
    s.rx = Some(rx);
    ports::watch(tx, move || waker());
}

fn poll(s: &mut State, now: f64, _: &mut Tx) -> bool {
    s.now = now;
    let mut changed = false;
    while let Some(result) = s.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
        s.apply(result);
        // A drive that was just plugged in or pulled changes the drive list too.
        s.refresh_drives();
        changed = true;
    }
    while let Some(msg) = s
        .drive
        .channel
        .as_ref()
        .and_then(|(_, rx)| rx.try_recv().ok())
    {
        s.drive_message(msg);
        changed = true;
    }
    changed
}

/// A cable that can't carry USB 3: the SSD from the example falls back to USB 2.
fn scene_slow(s: &mut State) {
    let mut scan = ports::example();
    let port = &mut scan.ports[1];
    let ssd = port.usb3.take().unwrap();
    port.usb2 = Some(ports::UsbLink {
        mbps: 480,
        rate: "480 Mbps (High Speed)".into(),
        ..ssd
    });
    port.active = vec!["CC".into(), "USB2".into()];
    s.scan = None;
    s.sessions.clear();
    s.apply(Ok(scan));
    s.port = Some(2);
}

fn scene_drawer(s: &mut State) {
    s.cable = TextEdit::new("White braided, 1 m");
    s.notes = TextEdit::new("Came with the SSD enclosure");
    s.save();
    scene_slow(s);
    s.cable = TextEdit::new("Black charging cable, 2 m");
    s.save();
    s.port = Some(1);
    s.cable = TextEdit::new("Monitor cable");
    s.save();
    s.message.clear();
    s.tab = Tab::Drawer;
}

fn main() {
    if std::env::args().any(|a| a == "--scan-json") {
        match ports::scan_once() {
            Ok(scan) => println!("{}", serde_json::to_string_pretty(&scan).unwrap()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    // `--speed-test [mount point]`: run the drive test without a window and print the result.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--speed-test") {
        let list = drives::list();
        let wanted = args.get(i + 1).map_or("/", String::as_str);
        let Some(drive) = list.iter().find(|d| d.mount == wanted) else {
            eprintln!("No testable drive at {wanted}. Found:");
            for d in &list {
                eprintln!("  {}  ({}, {})", d.mount, d.name, d.connection());
            }
            std::process::exit(1);
        };
        let Some(plan) = speed::Plan::for_free_space(drive.free) else {
            eprintln!("Less than 1 GB free on {}.", drive.name);
            std::process::exit(1);
        };
        println!("{} ({})", drive.name, drive.connection());
        match speed::run(&drive.test_dir, plan, &AtomicBool::new(false), |_| {}) {
            Ok(r) => {
                println!(
                    "Large files   read {:8.1} MB/s   write {:8.1} MB/s",
                    r.read, r.write
                );
                println!(
                    "Small pieces  read {:8.1} MB/s   write {:8.1} MB/s   ({:.0} and {:.0} a second)",
                    r.small_read, r.small_write, r.small_read_iops, r.small_write_iops
                );
                println!("{}", drives::verdict(drive, r.read));
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    rune::run(AppSpec {
        min_size: Some((1000.0, 700.0)),
        setup: Some(setup),
        poll: Some(poll),
        scenes: &[
            ("demo", |_| {}),
            ("monitor", |s| s.port = Some(1)),
            ("empty", |s| s.port = Some(3)),
            ("slow", scene_slow),
            ("drawer", scene_drawer),
            ("hub-empty", |s| {
                // A monitor with nothing plugged into its own ports, with the hub panel open.
                let mut scan = ports::example();
                scan.ports[0].behind.retain(|b| b.built_in);
                s.scan = None;
                s.apply(Ok(scan));
                s.port = Some(1);
                s.open = Some(Open::Hub);
            }),
            ("drives", |s| {
                s.tab = Tab::Drives;
                s.drive.selected = Some("/Volumes/Ugreen SSD".into());
                s.drive.results.insert(
                    "/Volumes/Ugreen SSD".into(),
                    speed::SpeedResult {
                        write: 912.4,
                        read: 968.7,
                        small_read: 38.2,
                        small_read_iops: 9_330.0,
                        small_write: 61.5,
                        small_write_iops: 15_020.0,
                        file_bytes: 1 << 30,
                    },
                );
            }),
            ("drives-running", |s| {
                s.tab = Tab::Drives;
                s.drive.running = Some("/".into());
                s.drive.live = [Some(5_210.0), Some(3_480.0), None, None];
                s.drive.progress = Some(speed::Progress {
                    phase: speed::Phase::Read,
                    fraction: 0.6,
                    mbps: 3_480.0,
                });
            }),
            ("guide", |s| s.tab = Tab::Guide),
            ("dark", |s| s.dark = Some(true)),
        ],
        ..AppSpec::new("Cable Check", (1200.0, 900.0), State::default, ui::view)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_needs_a_name_and_a_connected_port() {
        let mut s = State::default();
        assert!(!s.can_save());
        s.cable = TextEdit::new("Test");
        assert!(s.can_save());
        s.port = Some(3);
        assert!(!s.can_save());
        s.port = Some(2);
        s.load_error = Some("Unreadable".into());
        assert!(!s.can_save());
        s.load_error = None;
        s.save();
        assert_eq!(s.book.cables[0].name, "Test");
        assert_eq!(s.book.cables[0].tests[0].link, Some(Rung::Usb10));
        assert!(s.cable.text.is_empty());
    }

    #[test]
    fn a_newly_plugged_port_takes_the_bench() {
        let mut s = State::default();
        s.port = Some(1);
        let mut scan = ports::example();
        scan.ports[2].connected = true;
        scan.ports[2].connection_count += 1;
        s.apply(Ok(scan));
        assert_eq!(s.port, Some(3));
    }

    #[test]
    fn a_device_limit_pick_is_saved_and_cleared() {
        let mut s = State::default();
        assert_eq!(s.analysis().unwrap().limiter, verdict::Limiter::Device);
        // The SSD's chip is known to stop at 10 Gb/s, so a faster pick is set aside.
        s.set_device_limit(Some(Rung::Tb40));
        let a = s.analysis().unwrap();
        assert_eq!(a.device_limit.overruled, Some(Rung::Tb40));
        assert_eq!(a.limiter, verdict::Limiter::Device);
        s.set_device_limit(None);
        assert_eq!(s.analysis().unwrap().device_limit.overruled, None);
    }

    #[test]
    fn naming_a_saved_cable_brings_its_history_to_the_bench() {
        let mut s = State::default();
        s.cable = TextEdit::new("Grey");
        s.save();
        // The same cable on the monitor, wrongly told the monitor can do 10 Gb/s... which the
        // monitor's own report overrules; use the slow-link scene for a real failure instead.
        scene_slow(&mut s);
        assert_eq!(s.analysis().unwrap().limiter, verdict::Limiter::Cable);
        s.cable = TextEdit::new("grey");
        let a = s.analysis().unwrap();
        assert_eq!(a.limiter, verdict::Limiter::Device);
        assert_eq!(a.headline, "The device is the limit");
    }

    #[test]
    fn delete_asks_twice() {
        let mut s = State::default();
        scene_drawer(&mut s);
        assert_eq!(s.book.cables.len(), 3);
        s.delete_cable(0);
        assert_eq!(s.book.cables.len(), 3);
        s.delete_cable(0);
        assert_eq!(s.book.cables.len(), 2);
    }

    #[test]
    fn clicks_save_a_cable_and_open_the_drawer() {
        let mut state = State::default();
        state.cable = TextEdit::new("Braided");
        let mut rt = rune::Runtime::new(
            state,
            ui::view,
            (1200.0, 1400.0),
            rune::Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        let (x, y) = rt.find("save").expect("save button");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.1);
        assert_eq!(rt.state.book.cables.len(), 1);
        let (x, y) = rt.find("nav-drawer").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.1);
        assert_eq!(rt.state.tab, Tab::Drawer);
        assert!(rt.find("copy-json").is_some());
        assert!(rt.find("cable-0").is_some());
    }
}
