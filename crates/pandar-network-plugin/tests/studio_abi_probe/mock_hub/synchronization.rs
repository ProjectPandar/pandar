use std::{
    sync::mpsc::{Receiver, SyncSender, sync_channel},
    time::Instant,
};

pub(super) struct ProbeStart {
    sender: SyncSender<Instant>,
}

pub(super) struct HubStart {
    receiver: Receiver<Instant>,
}

pub(super) fn start_gate() -> (ProbeStart, HubStart) {
    let (sender, receiver) = sync_channel(0);
    (ProbeStart { sender }, HubStart { receiver })
}

impl ProbeStart {
    pub(super) fn arm(&self, deadline: Instant) {
        self.sender
            .send(deadline)
            .expect("start mock Hub after Studio ABI probe spawn");
    }
}

impl HubStart {
    pub(super) fn wait(self) -> Instant {
        self.receiver
            .recv()
            .expect("Studio ABI probe exited before starting the mock Hub")
    }
}
