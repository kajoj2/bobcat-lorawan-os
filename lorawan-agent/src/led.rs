// SPDX-License-Identifier: MIT
//! The front RGB LED shows the health verdict: green heartbeat when all is well (as the device tree leaves it),
//! blue without network, red when the gateway stack or the radio path is broken. While starting, the device tree's
//! green heartbeat stays.

use crate::status::Level;
use std::fs;
use std::path::PathBuf;

pub struct Led {
    red: Option<PathBuf>,
    green: Option<PathBuf>,
    blue: Option<PathBuf>,
    shown: Option<Level>,
}

fn find(color: &str) -> Option<PathBuf> {
    fs::read_dir("/sys/class/leds")
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{color}:"))
        })
        .map(|e| e.path())
}

fn set(led: &Option<PathBuf>, trigger: &str, on: bool) {
    if let Some(p) = led {
        let _ = fs::write(p.join("trigger"), trigger);
        if trigger == "none" {
            let max = fs::read_to_string(p.join("max_brightness")).unwrap_or_else(|_| "1".into());
            let _ = fs::write(p.join("brightness"), if on { max.trim() } else { "0" });
        }
    }
}

impl Led {
    pub fn new() -> Led {
        Led {
            red: find("red"),
            green: find("green"),
            blue: find("blue"),
            shown: None,
        }
    }

    pub fn present(&self) -> bool {
        self.red.is_some() || self.green.is_some() || self.blue.is_some()
    }

    pub fn show(&mut self, level: Level) {
        let level = if level == Level::Starting {
            Level::Ok
        } else {
            level
        };
        if self.shown == Some(level) {
            return;
        }
        match level {
            Level::Ok | Level::Starting => {
                set(&self.red, "none", false);
                set(&self.blue, "none", false);
                set(&self.green, "heartbeat", false);
            }
            Level::NoNetwork => {
                set(&self.green, "none", false);
                set(&self.red, "none", false);
                set(&self.blue, "none", true);
            }
            Level::Fault => {
                set(&self.green, "none", false);
                set(&self.blue, "none", false);
                set(&self.red, "none", true);
            }
        }
        self.shown = Some(level);
    }
}
