use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use swarm::color;

/// Tail a file and stream appended content.
pub(crate) fn tail_follow(
    path: &str,
    allow_missing: bool,
    stop: Option<Arc<AtomicBool>>,
) -> Result<(), String> {
    let mut offset: u64 = 0;

    loop {
        if let Some(flag) = stop.as_ref() {
            if flag.load(Ordering::SeqCst) {
                break;
            }
        }

        if !Path::new(path).exists() {
            if allow_missing {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            return Err(format!("{} not found", path));
        }

        let mut file = fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|e| format!("failed to open {}: {}", path, e))?;

        let len = file
            .metadata()
            .map_err(|e| format!("failed to stat {}: {}", path, e))?
            .len();
        if len < offset {
            offset = 0;
        }

        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("failed to seek {}: {}", path, e))?;

        let mut buffer = String::new();
        let bytes = file
            .read_to_string(&mut buffer)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;

        if bytes > 0 {
            // Colorize each line of the chat output
            for line in buffer.lines() {
                println!("{}", color::chat_line(line));
            }
            let _ = io::stdout().flush();
            offset += bytes as u64;
        }

        thread::sleep(Duration::from_millis(200));
    }

    Ok(())
}
