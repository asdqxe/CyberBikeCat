//! Explicitly labelled local events. No keyboard or agent capture.
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const STALE: Duration = Duration::from_secs(5);
const FEEDBACK: Duration = Duration::from_secs(2);
const DODGE: Duration = Duration::from_millis(900);
const BOOST: Duration = Duration::from_secs(2);

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn check_dir(dir: &Path, create: bool) -> io::Result<()> {
    if create {
        match DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    let metadata = fs::symlink_metadata(dir)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(invalid("event directory must be a real directory with permissions 0700"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Message {
    pub source: &'static str,
    pub state: &'static str,
}

impl Message {
    pub fn parse(text: &str) -> io::Result<Self> {
        let parts: Vec<_> = text.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(invalid("expected SOURCE STATE"));
        }
        let source = match parts[0] {
            "manual" => "MANUAL",
            "fake" => "FAKE",
            "preview" => "PREVIEW",
            "chat" => "CHAT",
            _ => return Err(invalid("source must be manual, fake, preview, or chat")),
        };
        let state = match parts[1] {
            "idle" => "IDLE",
            "busy" => "BUSY",
            "done" => "DONE",
            "error" => "ERROR",
            "cancelled" => "CANCELLED",
            "active" => "ACTIVE",
            "boost" => "BOOST",
            "left" => "LEFT",
            "right" => "RIGHT",
            "center" => "CENTER",
            _ => return Err(invalid("expected idle/busy/done/error/cancelled/active or boost/left/right/center")),
        };
        Ok(Self { source, state })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub direction: i8,
    pub boost: bool,
    pub steer_source: &'static str,
    pub boost_source: &'static str,
}

impl Motion {
    pub const NONE: Self = Self { direction: 0, boost: false, steer_source: "NONE", boost_source: "NONE" };
}

pub struct State {
    message: Message,
    received: Option<Instant>,
    steer: Option<(Message, Instant)>,
    boost: Option<(Message, Instant)>,
}

impl State {
    pub fn new() -> Self {
        Self { message: Message { source: "NONE", state: "DISCONNECTED" }, received: None, steer: None, boost: None }
    }

    pub fn receive(&mut self, message: Message, now: Instant) {
        match message.state {
            "LEFT" | "RIGHT" => { self.steer = Some((message, now)); return; }
            "BOOST" => { self.boost = Some((message, now)); return; }
            "CENTER" => { self.steer = None; return; }
            _ => {}
        }
        self.message = message;
        self.received = Some(now);
    }

    pub fn motion(&self, now: Instant) -> Motion {
        let steer = self.steer.filter(|(_, time)| now.saturating_duration_since(*time) < DODGE);
        let boost = self.boost.filter(|(_, time)| now.saturating_duration_since(*time) < BOOST);
        Motion {
            direction: steer.map_or(0, |(message, _)| if message.state == "LEFT" { -1 } else { 1 }),
            boost: boost.is_some(),
            steer_source: steer.map_or("NONE", |(message, _)| message.source),
            boost_source: boost.map_or("NONE", |(message, _)| message.source),
        }
    }

    pub fn snapshot(&self, now: Instant) -> Message {
        let elapsed = self.received.map_or(STALE, |received| now.saturating_duration_since(received));
        let state = if elapsed >= STALE {
            "DISCONNECTED"
        } else if matches!(self.message.state, "DONE" | "CANCELLED") && elapsed >= FEEDBACK {
            "IDLE"
        } else {
            self.message.state
        };
        if state == "DISCONNECTED" {
            let motion = self.motion(now);
            if motion.direction != 0 || motion.boost {
                return Message { source: if motion.direction != 0 { motion.steer_source } else { motion.boost_source }, state: "CONTROL" };
            }
        }
        Message { state, ..self.message }
    }
}

/// Deliberately small text vocabulary, not an LLM or arbitrary shell command.
pub fn text_action(text: &str) -> io::Result<&'static str> {
    match text.trim() {
        "boost" | "提速" | "加速" | "冲刺" => Ok("boost"),
        "left" | "左" | "向左" | "左闪" | "向左闪避" => Ok("left"),
        "right" | "右" | "向右" | "右闪" | "向右闪避" => Ok("right"),
        "center" | "回中" | "回正" => Ok("center"),
        _ => Err(invalid("supported text: 提速/加速/冲刺, 向左闪避, 向右闪避, 回中 (or boost/left/right/center)")),
    }
}

pub struct Feed {
    socket: UnixDatagram,
    path: PathBuf,
    pub state: State,
}

impl Feed {
    pub fn bind(dir: &Path) -> io::Result<Self> {
        check_dir(dir, true)?;
        let path = dir.join("events.sock");
        // Never unlink an existing endpoint: another viewer may own it.
        let socket = UnixDatagram::bind(&path)?;
        let feed = Self { socket, path, state: State::new() };
        fs::set_permissions(&feed.path, fs::Permissions::from_mode(0o600))?;
        feed.socket.set_nonblocking(true)?;
        Ok(feed)
    }

    pub fn poll(&mut self, now: Instant) -> io::Result<()> {
        // Bound work even if a sender floods the queue; oversized messages fail parsing.
        let mut buffer = [0u8; 64];
        for _ in 0..64 {
            match self.socket.recv(&mut buffer) {
                Ok(n) if n < buffer.len() => {
                    if let Ok(text) = std::str::from_utf8(&buffer[..n]) {
                        if let Ok(message) = Message::parse(text) {
                            self.state.receive(message, now);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn emit(dir: &Path, source: &str, state: &str) -> io::Result<()> {
    let wire = format!("{source} {state}");
    let message = Message::parse(&wire)?;
    let explain = |error: io::Error| {
        if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) {
            io::Error::new(error.kind(), format!(
                "no active viewer at {}; first run avcii-sketch view {} in another terminal and keep it open (demo does not receive events)",
                dir.join("events.sock").display(), dir.display()
            ))
        } else { error }
    };
    check_dir(dir, false).map_err(&explain)?;
    fs::symlink_metadata(dir.join("events.sock")).map_err(&explain)?;
    let socket = UnixDatagram::unbound()?;
    socket.set_write_timeout(Some(Duration::from_millis(250)))?;
    socket.send_to(wire.as_bytes(), dir.join("events.sock")).map_err(&explain)?;
    println!("sent {} {} (delivery queued, not an acknowledgement)", message.source, message.state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_feedback_expires_and_missing_heartbeat_disconnects() {
        let start = Instant::now();
        let mut state = State::new();
        assert_eq!(state.snapshot(start).state, "DISCONNECTED");
        state.receive(Message::parse("fake done").unwrap(), start);
        assert_eq!(state.snapshot(start + Duration::from_secs(1)).state, "DONE");
        assert_eq!(state.snapshot(start + Duration::from_secs(2)).state, "IDLE");
        assert_eq!(state.snapshot(start + Duration::from_secs(5)).state, "DISCONNECTED");
        state.receive(Message::parse("manual busy").unwrap(), start);
        assert_eq!(state.snapshot(start + Duration::from_secs(4)).state, "BUSY");
        assert_eq!(state.snapshot(start + Duration::from_secs(5)).state, "DISCONNECTED");
    }

    #[test]
    fn reject_unlabelled_or_unknown_events() {
        for bad in ["busy", "real busy", "fake thinking", "fake done extra", ""] {
            assert!(Message::parse(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn actions_expire_without_overwriting_task_state_or_heartbeat() {
        let start = Instant::now();
        let mut state = State::new();
        state.receive(Message::parse("fake busy").unwrap(), start);
        state.receive(Message::parse("manual left").unwrap(), start);
        state.receive(Message::parse("manual boost").unwrap(), start);
        assert_eq!(state.snapshot(start).state, "BUSY");
        assert_eq!(state.snapshot(start).source, "FAKE");
        assert_eq!(state.motion(start).direction, -1);
        assert!(state.motion(start).boost);
        assert_eq!(state.motion(start + DODGE).direction, 0);
        assert!(!state.motion(start + BOOST).boost);
        state.receive(Message::parse("manual right").unwrap(), start + STALE);
        assert_eq!(state.snapshot(start + STALE).state, "CONTROL");
        state.receive(Message::parse("manual center").unwrap(), start + STALE);
        assert_eq!(state.snapshot(start + STALE).state, "DISCONNECTED");
    }

    #[test]
    fn text_commands_are_explicit_and_do_not_execute_free_text() {
        assert_eq!(text_action(" 向左闪避 ").unwrap(), "left");
        assert_eq!(text_action("提速").unwrap(), "boost");
        assert_eq!(text_action("向右闪避").unwrap(), "right");
        assert!(text_action("不要向左").is_err());
        assert!(text_action("left; echo unexpected").is_err());
    }
}
