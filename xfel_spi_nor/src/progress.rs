//! Progress display matching xfel `progress.c` (stderr, TTY only).

use std::io::{IsTerminal, Write};
use std::time::Instant;

const BAR_WIDTH: usize = 48;

pub struct Progress {
    total: u64,
    done: u64,
    start: Instant,
}

impl Progress {
    pub fn start(total: u64) -> Option<Self> {
        if total == 0 || !std::io::stderr().is_terminal() {
            return None;
        }
        Some(Self {
            total,
            done: 0,
            start: Instant::now(),
        })
    }

    pub fn update(&mut self, bytes: u64) {
        self.done += bytes;
        let elapsed = self.start.elapsed().as_secs_f64().max(1e-9);
        let ratio = if self.total > 0 {
            self.done as f64 / self.total as f64
        } else {
            0.0
        };
        let speed = self.done as f64 / elapsed;
        let eta = if speed > 0.0 {
            (self.total.saturating_sub(self.done)) as f64 / speed
        } else {
            0.0
        };
        let pos = (BAR_WIDTH as f64 * ratio).min(BAR_WIDTH as f64) as usize;
        let bar: String = (0..BAR_WIDTH)
            .map(|i| if i < pos { '=' } else { ' ' })
            .collect();
        let mut err = std::io::stderr();
        let _ = write!(err, "\r{:3.0}% [{}]", ratio * 100.0, bar);
        if self.done < self.total {
            let _ = write!(
                err,
                " {} /s, ETA {}        ",
                format_size(speed),
                format_eta(eta)
            );
        } else {
            let _ = write!(
                err,
                " {}, {} /s        ",
                format_size(self.done as f64),
                format_size(speed)
            );
        }
        let _ = err.flush();
    }

    pub fn stop(self) {
        let _ = writeln!(std::io::stderr());
    }
}

fn format_eta(remaining: f64) -> String {
    let seconds = (remaining + 0.5) as i64;
    if (0..6000).contains(&seconds) {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    } else {
        "--:--".into()
    }
}

fn format_size(size: f64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let mut size = size;
    let mut count = 0usize;
    while size > 1024.0 && count < 8 {
        size /= 1024.0;
        count += 1;
    }
    format!("{:5.3} {}", size, UNITS[count])
}
