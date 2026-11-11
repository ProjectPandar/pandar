use std::{
    collections::HashMap,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::sync::oneshot;

struct PausePoint {
    generation: u64,
    reached: oneshot::Sender<()>,
    resume: oneshot::Receiver<()>,
}

pub(crate) struct OwnershipPause {
    printer_id: String,
    generation: u64,
    reached: oneshot::Receiver<()>,
    resume: Option<oneshot::Sender<()>>,
}

static PAUSES: OnceLock<std::sync::Mutex<HashMap<String, PausePoint>>> = OnceLock::new();
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn install(printer_id: &str) -> OwnershipPause {
    let (reached_sender, reached_receiver) = oneshot::channel();
    let (resume_sender, resume_receiver) = oneshot::channel();
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let mut pauses = pauses()
        .lock()
        .expect("printer ownership pause mutex should not be poisoned");
    if pauses.contains_key(printer_id) {
        drop(pauses);
        panic!("printer ownership pause already installed for {printer_id}");
    }
    pauses.insert(
        printer_id.to_owned(),
        PausePoint {
            generation,
            reached: reached_sender,
            resume: resume_receiver,
        },
    );
    OwnershipPause {
        printer_id: printer_id.to_owned(),
        generation,
        reached: reached_receiver,
        resume: Some(resume_sender),
    }
}

impl OwnershipPause {
    pub(crate) async fn wait_until_reached(self) -> Result<oneshot::Sender<()>, &'static str> {
        self.wait_until_reached_with_timeout(WAIT_TIMEOUT).await
    }

    async fn wait_until_reached_with_timeout(
        mut self,
        timeout: Duration,
    ) -> Result<oneshot::Sender<()>, &'static str> {
        match tokio::time::timeout(timeout, &mut self.reached).await {
            Ok(Ok(())) => Ok(self
                .resume
                .take()
                .expect("printer ownership pause resume sender must be present")),
            Ok(Err(_)) => Err("printer operation ownership pause was dropped"),
            Err(_) => Err("timed out waiting for printer operation ownership pause"),
        }
    }
}

impl Drop for OwnershipPause {
    fn drop(&mut self) {
        remove_if_current(&self.printer_id, self.generation);
    }
}

pub(super) async fn wait(printer_id: &str) {
    let pause = pauses()
        .lock()
        .expect("printer ownership pause mutex should not be poisoned")
        .remove(printer_id);
    if let Some(pause) = pause {
        let _ = pause.reached.send(());
        let _ = pause.resume.await;
    }
}

fn pauses() -> &'static std::sync::Mutex<HashMap<String, PausePoint>> {
    PAUSES.get_or_init(Default::default)
}

fn remove_if_current(printer_id: &str, generation: u64) {
    let mut pauses = pauses()
        .lock()
        .expect("printer ownership pause mutex should not be poisoned");
    if pauses
        .get(printer_id)
        .is_some_and(|pause| pause.generation == generation)
    {
        pauses.remove(printer_id);
    }
}
