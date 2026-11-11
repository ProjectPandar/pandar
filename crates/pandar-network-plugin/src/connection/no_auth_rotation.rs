use std::{ffi::c_void, time::Duration};

use crate::{cancellation::RequestCancellation, read_utf8};

use super::{ConnectionSession, ffi::session};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct NoAuthRotationKey {
    hub_url: String,
    token: String,
    account_epoch: u64,
    config_epoch: u64,
}

impl NoAuthRotationKey {
    pub(crate) fn new(
        hub_url: String,
        token: String,
        account_epoch: u64,
        config_epoch: u64,
    ) -> Self {
        Self {
            hub_url,
            token,
            account_epoch,
            config_epoch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NoAuthRotationOutcome {
    pub(crate) status: i32,
    pub(crate) http_code: u32,
    pub(crate) body: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NoAuthRotationBegin {
    Started,
    Finished(NoAuthRotationOutcome),
    NotApplicable,
    Cancelled,
}

#[derive(Default)]
pub(super) enum NoAuthRotation {
    #[default]
    Idle,
    InFlight(NoAuthRotationKey),
    Finished(NoAuthRotationKey, NoAuthRotationOutcome),
}

impl ConnectionSession {
    pub(crate) fn no_auth_rotation_in_flight(&self) -> bool {
        matches!(
            self.state
                .lock()
                .expect("connection state")
                .no_auth_rotation,
            NoAuthRotation::InFlight(_)
        )
    }

    #[cfg(test)]
    pub(crate) fn begin_no_auth_rotation(&self, key: NoAuthRotationKey) -> NoAuthRotationBegin {
        self.begin_no_auth_rotation_before_wait(key, RequestCancellation::disabled(), || {})
    }

    pub(crate) fn begin_no_auth_rotation_cancellable(
        &self,
        key: NoAuthRotationKey,
        cancellation: RequestCancellation,
    ) -> NoAuthRotationBegin {
        self.begin_no_auth_rotation_before_wait(key, cancellation, || {})
    }

    fn begin_no_auth_rotation_before_wait(
        &self,
        key: NoAuthRotationKey,
        cancellation: RequestCancellation,
        before_wait: impl FnOnce(),
    ) -> NoAuthRotationBegin {
        let mut before_wait = Some(before_wait);
        let mut state = self.state.lock().expect("connection state");
        loop {
            if cancellation.is_cancelled() {
                return NoAuthRotationBegin::Cancelled;
            }
            match &state.no_auth_rotation {
                NoAuthRotation::InFlight(active) if active == &key => {
                    if let Some(before_wait) = before_wait.take() {
                        before_wait();
                    }
                    (state, _) = self
                        .no_auth_rotation_changed
                        .wait_timeout(state, Duration::from_millis(10))
                        .expect("connection state");
                }
                NoAuthRotation::Finished(active, outcome) if active == &key => {
                    return NoAuthRotationBegin::Finished(outcome.clone());
                }
                NoAuthRotation::Idle
                | NoAuthRotation::InFlight(_)
                | NoAuthRotation::Finished(_, _) => {
                    if state.hub_url != key.hub_url
                        || state.token != key.token
                        || state.account_epoch != key.account_epoch
                    {
                        return NoAuthRotationBegin::NotApplicable;
                    }
                    match state.no_auth_rotation {
                        NoAuthRotation::Idle | NoAuthRotation::Finished(_, _) => {
                            state.no_auth_rotation = NoAuthRotation::InFlight(key);
                            return NoAuthRotationBegin::Started;
                        }
                        NoAuthRotation::InFlight(_) => {
                            return NoAuthRotationBegin::NotApplicable;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn finish_no_auth_rotation(
        &self,
        key: NoAuthRotationKey,
        outcome: NoAuthRotationOutcome,
    ) -> bool {
        let mut state = self.state.lock().expect("connection state");
        if !matches!(
            &state.no_auth_rotation,
            NoAuthRotation::InFlight(active) if active == &key
        ) {
            return false;
        }
        state.no_auth_rotation = NoAuthRotation::Finished(key, outcome);
        drop(state);
        self.no_auth_rotation_changed.notify_all();
        true
    }

    fn claim_no_auth_rotation(&self, key: NoAuthRotationKey) -> bool {
        let mut state = self.state.lock().expect("connection state");
        if state.hub_url != key.hub_url
            || state.token != key.token
            || state.account_epoch != key.account_epoch
            || matches!(
                &state.no_auth_rotation,
                NoAuthRotation::InFlight(active) | NoAuthRotation::Finished(active, _)
                    if active == &key
            )
        {
            return false;
        }
        state.no_auth_rotation = NoAuthRotation::InFlight(key);
        drop(state);
        self.no_auth_rotation_changed.notify_all();
        true
    }
}

#[cfg(test)]
mod cancellation_tests;

#[unsafe(no_mangle)]
/// # Safety
/// Handles must be live, byte inputs valid for paired lengths, outputs writable, and callback contexts valid for the call.
pub unsafe extern "C" fn pandar_plugin_no_auth_rotation_claim(
    session_ptr: *mut c_void,
    account_epoch: u64,
    config_epoch: u64,
    hub_url_ptr: *const u8,
    hub_url_len: usize,
    token_ptr: *const u8,
    token_len: usize,
) -> i32 {
    let Some(session) = (unsafe { session(session_ptr) }) else {
        return 0;
    };
    let Some(hub_url) = (unsafe { read_utf8(hub_url_ptr, hub_url_len) }) else {
        return 0;
    };
    let Some(token) = unsafe { read_utf8(token_ptr, token_len) }.filter(|token| !token.is_empty())
    else {
        return 0;
    };
    session.claim_no_auth_rotation(NoAuthRotationKey::new(
        hub_url.to_owned(),
        token.to_owned(),
        account_epoch,
        config_epoch,
    )) as i32
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use super::*;

    fn key(token: &str, config_epoch: u64) -> NoAuthRotationKey {
        NoAuthRotationKey::new("http://hub".to_owned(), token.to_owned(), 0, config_epoch)
    }

    #[test]
    fn same_key_waiter_replays_after_the_session_commits_a_replacement_token() {
        let session = Arc::new(ConnectionSession::new(
            "http://hub".to_owned(),
            "old-token".to_owned(),
        ));
        let rotation_key = key("old-token", 7);
        let success = NoAuthRotationOutcome {
            status: 0,
            http_code: 200,
            body: r#"{"access_token":"replacement-token"}"#.to_owned(),
        };
        assert_eq!(
            session.begin_no_auth_rotation(rotation_key.clone()),
            NoAuthRotationBegin::Started
        );

        let (waiting_tx, waiting_rx) = mpsc::channel();
        let waiter_session = Arc::clone(&session);
        let waiter_key = rotation_key.clone();
        let waiter = thread::spawn(move || {
            waiter_session.begin_no_auth_rotation_before_wait(
                waiter_key,
                RequestCancellation::disabled(),
                || waiting_tx.send(()).expect("announce wait"),
            )
        });
        waiting_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("waiter reached in-flight state");

        session.update("http://hub".to_owned(), "replacement-token".to_owned());
        assert!(session.finish_no_auth_rotation(rotation_key, success.clone()));
        assert_eq!(
            waiter.join().expect("waiter result"),
            NoAuthRotationBegin::Finished(success)
        );
    }
}
