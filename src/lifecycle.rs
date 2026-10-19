//! Connection state and retry policy, independent of sockets and wall clocks.
use bootleg_core::{ContainerId, SmugglerSessionId, api::ErrorCode};
use std::{collections::VecDeque, time::Duration};

#[derive(Debug, PartialEq, Eq)]
pub enum Phase {
    Disconnected,
    Registering {
        deadline: Duration,
    },
    Connected {
        session: SmugglerSessionId,
        next_heartbeat: Duration,
    },
    AwaitingHeartbeat {
        session: SmugglerSessionId,
        deadline: Duration,
    },
    Disabled,
}
pub struct Participation {
    pub container: ContainerId,
    pub phase: Phase,
    attempt: u32,
}
impl Participation {
    pub fn new(container: ContainerId) -> Self {
        Self {
            container,
            phase: Phase::Disconnected,
            attempt: 0,
        }
    }
    pub fn begin(&mut self, now: Duration) -> Result<(), &'static str> {
        if self.phase != Phase::Disconnected {
            return Err("connection already active or disabled");
        }
        self.phase = Phase::Registering {
            deadline: now + Duration::from_secs(5),
        };
        Ok(())
    }
    pub fn registered(
        &mut self,
        session: SmugglerSessionId,
        now: Duration,
        interval: u64,
        lease: u64,
    ) -> Result<(), &'static str> {
        if !matches!(self.phase, Phase::Registering { deadline } if now < deadline)
            || interval != 15
            || lease != 45
        {
            return Err("unexpected registration acknowledgment");
        }
        self.phase = Phase::Connected {
            session,
            next_heartbeat: now + Duration::from_secs(15),
        };
        Ok(())
    }
    pub fn heartbeat(&mut self, now: Duration) -> Result<SmugglerSessionId, &'static str> {
        match &self.phase {
            Phase::Connected {
                session,
                next_heartbeat,
            } if now >= *next_heartbeat => {
                let session = session.clone();
                self.phase = Phase::AwaitingHeartbeat {
                    session: session.clone(),
                    deadline: now + Duration::from_secs(15),
                };
                Ok(session)
            }
            _ => Err("heartbeat not due"),
        }
    }
    pub fn acknowledged(
        &mut self,
        id: &SmugglerSessionId,
        now: Duration,
    ) -> Result<(), &'static str> {
        match &self.phase {
            Phase::AwaitingHeartbeat { session, deadline } if session == id && now < *deadline => {
                self.phase = Phase::Connected {
                    session: id.clone(),
                    next_heartbeat: now + Duration::from_secs(15),
                };
                self.attempt = 0;
                Ok(())
            }
            _ => Err("unexpected heartbeat acknowledgment"),
        }
    }
    pub fn disconnected(&mut self) {
        if self.phase != Phase::Disabled {
            self.phase = Phase::Disconnected;
        }
    }
    pub fn failure(&mut self, code: ErrorCode) {
        if terminal(code) {
            self.phase = Phase::Disabled;
        } else {
            self.disconnected();
        }
    }
    /// entropy in [0,1], full jitter; server delay is always a minimum.
    pub fn retry_delay(&mut self, entropy: f64, minimum: u64) -> Duration {
        let cap = (1u64 << self.attempt.min(5)).min(30);
        self.attempt = self.attempt.saturating_add(1);
        let entropy = if entropy.is_finite() {
            entropy.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Duration::from_secs_f64(cap as f64 * entropy).max(Duration::from_secs(minimum))
    }
}
pub fn terminal(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Unauthorized
            | ErrorCode::Forbidden
            | ErrorCode::NotFound
            | ErrorCode::ProtocolIncompatible
            | ErrorCode::Validation
    )
}
#[derive(Default)]
pub struct NoticeMemory(VecDeque<String>);
impl NoticeMemory {
    pub fn first_seen(&mut self, id: &str) -> bool {
        if self.0.iter().any(|s| s == id) {
            return false;
        }
        if self.0.len() == 128 {
            self.0.pop_front();
        }
        self.0.push_back(id.to_owned());
        true
    }
}
/// Neutralize terminal and Unicode directional formatting, without interpreting text.
pub fn diagnostic_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() && !matches!(*c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).collect()
}
