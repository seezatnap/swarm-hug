use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc::Sender, Arc};
use std::thread;
use std::time::Duration;

use super::message::TuiMessage;
use crate::chat;

/// Tail a chat file and send lines to the TUI.
pub(super) fn tail_chat_to_tui(path: &str, tx: Sender<TuiMessage>, stop: Arc<AtomicBool>) {
    use std::fs::File;
    use std::io::{BufReader, Read, Seek, SeekFrom};

    let mut offset: u64 = 0;

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        let len = match file.metadata() {
            Ok(m) => m.len(),
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        if len < offset {
            offset = 0;
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(offset)).is_err() {
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        let mut new_content = String::new();
        let bytes_read = reader.read_to_string(&mut new_content).unwrap_or(0);

        if bytes_read > 0 {
            for line in new_content.lines() {
                if !line.is_empty() && !chat::is_heartbeat_line(line) {
                    let colored_line = crate::color::chat_line(line);
                    if tx.send(TuiMessage::AppendLine(colored_line)).is_err() {
                        return;
                    }
                }
            }
            offset += bytes_read as u64;
        }

        thread::sleep(Duration::from_millis(100));
    }
}
