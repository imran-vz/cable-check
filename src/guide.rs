//! The speed ladder and every piece of explanatory copy. Kept in one file so the wording can be
//! read and edited in one sitting.
use serde::{Deserialize, Serialize};

/// A step on the data-speed ladder a USB-C cable can be graded against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Rung {
    Usb2,
    Usb5,
    Usb10,
    Usb20,
    Tb40,
    Tb80,
}

pub const LADDER: [Rung; 6] = [
    Rung::Usb2,
    Rung::Usb5,
    Rung::Usb10,
    Rung::Usb20,
    Rung::Tb40,
    Rung::Tb80,
];

impl Rung {
    pub fn mbps(self) -> u64 {
        match self {
            Rung::Usb2 => 480,
            Rung::Usb5 => 5_000,
            Rung::Usb10 => 10_000,
            Rung::Usb20 => 20_000,
            Rung::Tb40 => 40_000,
            Rung::Tb80 => 80_000,
        }
    }
    /// The fastest rung a link of `mbps` fully reaches.
    pub fn from_mbps(mbps: u64) -> Option<Rung> {
        LADDER.iter().rev().copied().find(|r| mbps >= r.mbps())
    }
    pub fn speed(self) -> &'static str {
        match self {
            Rung::Usb2 => "480 Mb/s",
            Rung::Usb5 => "5 Gb/s",
            Rung::Usb10 => "10 Gb/s",
            Rung::Usb20 => "20 Gb/s",
            Rung::Tb40 => "40 Gb/s",
            Rung::Tb80 => "80 Gb/s",
        }
    }
    /// The current official name.
    pub fn name(self) -> &'static str {
        match self {
            Rung::Usb2 => "USB 2.0",
            Rung::Usb5 => "USB 3.2 Gen 1",
            Rung::Usb10 => "USB 3.2 Gen 2",
            Rung::Usb20 => "USB 3.2 Gen 2x2",
            Rung::Tb40 => "USB4 / Thunderbolt 3 and 4",
            Rung::Tb80 => "USB4 v2 / Thunderbolt 5",
        }
    }
    /// Names the same speed has been sold under.
    pub fn also_called(self) -> &'static str {
        match self {
            Rung::Usb2 => "Hi-Speed USB",
            Rung::Usb5 => "USB 3.0, USB 3.1 Gen 1, SuperSpeed",
            Rung::Usb10 => "USB 3.1 Gen 2, SuperSpeed+",
            Rung::Usb20 => "SuperSpeed 20Gbps",
            Rung::Tb40 => "USB4 Gen 3, USB 40Gbps",
            Rung::Tb80 => "USB4 Gen 4, USB 80Gbps",
        }
    }
    /// What the speed means when copying files.
    pub fn in_practice(self) -> &'static str {
        match self {
            Rung::Usb2 => "About 40 MB/s. A 50 GB copy takes 20 minutes or more.",
            Rung::Usb5 => "About 400 MB/s. A 50 GB copy takes around 2 minutes.",
            Rung::Usb10 => "About 1,000 MB/s. A 50 GB copy takes under a minute.",
            Rung::Usb20 => "About 2,000 MB/s on PCs that support it. Macs don't.",
            Rung::Tb40 => "About 3,000 MB/s with a fast SSD. 50 GB in under 20 seconds.",
            Rung::Tb80 => "6,000 MB/s and up, if the SSD can keep pace.",
        }
    }
    /// The long tooltip for a rung.
    pub fn help(self) -> &'static str {
        match self {
            Rung::Usb2 => {
                "USB 2.0, also sold as Hi-Speed USB. It tops out at 480 Mb/s, which works out to about 40 MB/s when copying files.\n\nThat is plenty for charging, keyboards, mice and audio. It is slow for big files: 50 GB takes 20 minutes or more.\n\nA lot of charging cables only have USB 2 data wires inside, including some thick, expensive ones."
            }
            Rung::Usb5 => {
                "USB 3.2 Gen 1, 5 Gb/s. The same speed has been called USB 3.0, USB 3.1 Gen 1 and SuperSpeed, which is why the labels on cables are such a mess.\n\nExpect about 400 MB/s when copying files. It needs the extra high-speed wires that USB 2 cables leave out."
            }
            Rung::Usb10 => {
                "USB 3.2 Gen 2, 10 Gb/s. Earlier names: USB 3.1 Gen 2 and SuperSpeed+.\n\nExpect about 1,000 MB/s. Most NVMe SSD enclosures stop here, and so do the Pro iPhones."
            }
            Rung::Usb20 => {
                "USB 3.2 Gen 2x2 runs two 10 Gb/s lanes side by side to reach 20 Gb/s.\n\nMacs don't support it. A 20 Gb/s drive links at 10 Gb/s on a Mac, so this app can't confirm 20 Gb/s for any cable. A cable that passes 40 Gb/s will carry it."
            }
            Rung::Tb40 => {
                "USB4 and Thunderbolt 3 and 4 run at 40 Gb/s. A fast SSD manages about 3,000 MB/s over it.\n\nThe cable needs a chip (an e-marker) that says it can handle this speed. Passive cables that do are usually short, a metre or less. Longer ones have signal boosters built in and cost more."
            }
            Rung::Tb80 => {
                "Thunderbolt 5 and USB4 version 2 run at 80 Gb/s in each direction. They can also switch to 120 Gb/s one way for high-resolution displays.\n\nOnly recent Macs (M4 Pro and M4 Max onward) and a small number of docks and SSDs support it so far."
            }
        }
    }
    /// Devices a person might own that can reach this rung, for when the current one can't.
    pub fn testers(self) -> &'static str {
        match self {
            Rung::Usb2 => "any phone or USB drive",
            Rung::Usb5 => "any USB 3 flash drive or SSD",
            Rung::Usb10 => "a 10 Gb/s NVMe enclosure, or an iPhone 15 Pro or later Pro",
            Rung::Usb20 => "a PC with a 20 Gb/s port (no Mac has one)",
            Rung::Tb40 => "a USB4 or Thunderbolt SSD, a Thunderbolt dock, or a second Mac",
            Rung::Tb80 => "a Thunderbolt 5 dock, SSD or Mac",
        }
    }
    /// What kinds of device usually top out at this rung, for the device-limit picker.
    pub fn typical_devices(self) -> &'static str {
        match self {
            Rung::Usb2 => {
                "Pick this for USB 2 devices: iPhone 15 and later that aren't Pro models, a lot of Android phones, keyboards, mice, audio gear, and most chargers and power banks."
            }
            Rung::Usb5 => {
                "Pick this for 5 Gb/s devices: portable hard drives, SATA SSD enclosures, most USB 3 flash drives and hubs, and many Android phones. Check the spec sheet if you can."
            }
            Rung::Usb10 => {
                "Pick this for 10 Gb/s devices: most NVMe SSD enclosures, portable SSDs like the Samsung T7, and iPhone 15 Pro or later Pro models."
            }
            Rung::Usb20 => {
                "Pick this for 20 Gb/s (Gen 2x2) SSDs. A Mac will only link to them at 10 Gb/s."
            }
            Rung::Tb40 => {
                "Pick this for USB4 and Thunderbolt 3 or 4 gear: Thunderbolt docks and SSDs, USB4 enclosures, Studio Display, iPad Pro with an M-series chip, or another Mac."
            }
            Rung::Tb80 => "Pick this for Thunderbolt 5 docks, SSDs and Macs.",
        }
    }
}

/// A term with a plain explanation, shown as a tooltip and on the guide page.
#[derive(Debug, PartialEq)]
pub struct Term {
    pub name: &'static str,
    pub text: &'static str,
}

pub const EMARKER: Term = Term {
    name: "E-marker",
    text: "A tiny chip inside the plug of some USB-C cables. It tells the computer and charger what the cable is built for: top data speed, how much current it can carry, and whether it has signal boosters.\n\nCables rated for more than 60 W, or for USB4 and Thunderbolt, must have one. Plain charging cables usually don't.\n\nThe chip only states what the maker programmed into it. This app reads that claim and then checks it against what the link really does.",
};
pub const LINK_SPEED: Term = Term {
    name: "Link speed",
    text: "When you plug in, the two devices try the fastest mode they both support. If the cable can't carry that signal cleanly they fall back to a slower one. The speed they settle on is the link speed.\n\nIt is the ceiling for this connection, not a measured file-copy speed. Real copies run below it because of overhead and the drive itself.",
};
pub const BITS_BYTES: Term = Term {
    name: "Gb/s and MB/s",
    text: "Cables and ports are rated in bits per second (Mb/s, Gb/s). File copies are shown in bytes per second (MB/s). There are 8 bits in a byte, and USB spends some of the link on overhead.\n\nRule of thumb: divide Gb/s by 10 to get GB/s. A 10 Gb/s link copies at roughly 1 GB/s (1,000 MB/s) at best.",
};
pub const POWER_DELIVERY: Term = Term {
    name: "Power Delivery (PD)",
    text: "The way USB-C chargers and devices agree on a voltage and current. Without it USB-C gives 5 V only.\n\nAny USB-C cable may carry 3 A, which is 60 W at 20 V. Going higher needs a 5 A cable with an e-marker: 100 W at 20 V, or up to 240 W with the newer 28 to 48 V modes (EPR).\n\nA charger checks the cable first. If the cable isn't marked for 5 A, the charger caps its offer at 3 A.",
};
pub const ALT_MODE: Term = Term {
    name: "Video over USB-C",
    text: "USB-C can carry a DisplayPort video signal on the same high-speed wires USB 3 uses. This is called DisplayPort Alt Mode.\n\nA cable that only has USB 2 wires can't carry video at all, no matter what the monitor or laptop supports.",
};
pub const ORIENTATION: Term = Term {
    name: "Both ways up",
    text: "A USB-C plug works either way up, but USB 3 uses a different set of wires for each side. A cable with one damaged wire can run at full speed one way and drop to USB 2 the other way.\n\nTo be sure, test once, pull the plug out of the Mac, turn it over, and test again. USB4 and Thunderbolt use all the wires at once, so one test covers both sides.",
};
pub const PASSIVE_ACTIVE: Term = Term {
    name: "Passive and active cables",
    text: "A passive cable is just wire. The faster the signal, the shorter it has to be: about 2 m for 5 Gb/s, 1 m for 10 Gb/s, and under 1 m for 40 Gb/s.\n\nAn active cable has chips in the plugs that clean up the signal, so it can be longer. A long cable that claims a high speed and says it is passive deserves suspicion.",
};
pub const BOTTLENECK: Term = Term {
    name: "Why the slowest part wins",
    text: "A link has three parts: the Mac's port, the cable, and the device. It runs at the speed of the slowest one.\n\nSo a test only tells you about the cable up to the speed of the slower end. A cable that links at 10 Gb/s to a 10 Gb/s drive has passed everything that drive can ask of it. Whether it could do 40 Gb/s is still unknown until you try it with a 40 Gb/s device.",
};
pub const STABILITY: Term = Term {
    name: "Stability",
    text: "A marginal cable sometimes links at full speed and then drops to a slower mode, or disconnects and reconnects. This app watches the link once a second while the cable is plugged in and counts every change.\n\nLeave it for a minute and wiggle the plugs gently. A good cable stays put.",
};

pub const HUB: Term = Term {
    name: "Hubs, and the USB ports on a monitor",
    text: "A hub turns one USB connection into several ports. Monitors and docks that have USB ports have a hub built in.\n\nA fast hub is really two hubs in one box: a USB 2 one for things like keyboards and mice, and a USB 3 one for drives. That is why macOS lists two entries with names like \"USB2.1 Hub\" and \"USB3.2 Hub\" when you plug in a single monitor.\n\nEverything plugged into the hub shares the one cable back to the Mac, and can't go faster than the hub itself.",
};

pub const LARGE_FILES: Term = Term {
    name: "Large files",
    text: "How fast the drive moves one big file from start to finish, like a video or a disk backup. This is the number on the drive's box.\n\nThe test writes a file of up to 1 GB, then reads it back, and times both.",
};
pub const SMALL_PIECES: Term = Term {
    name: "Small pieces",
    text: "How fast the drive handles thousands of tiny reads and writes scattered around, one at a time. Opening apps, searching, and folders full of small files work this way.\n\nThese numbers are always far lower than the large-file ones. Hard drives with spinning disks are especially slow here.",
};

pub const GLOSSARY: [&Term; 12] = [
    &BOTTLENECK,
    &HUB,
    &LARGE_FILES,
    &SMALL_PIECES,
    &LINK_SPEED,
    &BITS_BYTES,
    &EMARKER,
    &POWER_DELIVERY,
    &ALT_MODE,
    &ORIENTATION,
    &PASSIVE_ACTIVE,
    &STABILITY,
];

/// USB bridge chips common in SSD enclosures, by USB vendor and product ID, with the fastest
/// link each supports.
pub fn known_chip(vid: u16, pid: u16) -> Option<(&'static str, Rung)> {
    Some(match (vid, pid) {
        (0x0bda, 0x9210) => ("Realtek RTL9210 NVMe bridge", Rung::Usb10),
        (0x152d, 0x0583) => ("JMicron JMS583 NVMe bridge", Rung::Usb10),
        (0x174c, 0x2362) => ("ASMedia ASM2362 NVMe bridge", Rung::Usb10),
        (0x174c, 0x2364) => ("ASMedia ASM2364 NVMe bridge", Rung::Usb20),
        (0x152d, 0x0578) => ("JMicron JMS578 SATA bridge", Rung::Usb5),
        (0x174c, 0x1153) => ("ASMedia ASM1153 SATA bridge", Rung::Usb5),
        (0x174c, 0x2463) => ("ASMedia ASM2464PD USB4 bridge", Rung::Tb40),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rung_from_link_rate_rounds_down() {
        assert_eq!(Rung::from_mbps(12), None);
        assert_eq!(Rung::from_mbps(480), Some(Rung::Usb2));
        assert_eq!(Rung::from_mbps(5_000), Some(Rung::Usb5));
        assert_eq!(Rung::from_mbps(10_000), Some(Rung::Usb10));
        assert_eq!(Rung::from_mbps(40_000), Some(Rung::Tb40));
        assert_eq!(Rung::from_mbps(120_000), Some(Rung::Tb80));
    }
}
