//! The pre-annotation worker the server starts itself, when the worker settings give a command:
//! run through the shell in a process group of its own, started again when it exits, and stopped
//! (with everything it started) when the settings change or the server stops.

use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

/// Process groups of the workers running now, in any server of this process, for `stop_workers`.
static GROUPS: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());

/// Stops every worker the server started; for when the server stops.
pub fn stop_workers() {
    for group in std::mem::take(&mut *GROUPS.lock().expect("worker groups lock")) {
        kill_group(group);
    }
}

fn kill_group(group: i32) {
    // SAFETY: kill only sends a signal; a negative pid names the process group.
    unsafe { libc::kill(-group, libc::SIGTERM) };
}

/// Runs at most one worker command at a time.
#[derive(Clone, Default)]
pub struct WorkerProcess {
    /// Bumped whenever the command changes; a supervising loop of an older one ends.
    generation: Arc<AtomicU64>,
    group: Arc<Mutex<Option<i32>>>,
}

impl WorkerProcess {
    /// Stops the running worker, if any, and starts `command` (if given) listening at `addr`
    /// (`host:port`, passed as `OXILABEL_WORKER_ADDR`).
    pub fn run(&self, command: Option<String>, addr: String) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(group) = self.group.lock().expect("worker group lock").take() {
            GROUPS.lock().expect("worker groups lock").remove(&group);
            kill_group(group);
        }
        let Some(command) = command else { return };
        let this = self.clone();
        tokio::spawn(async move {
            while this.generation.load(Ordering::SeqCst) == generation {
                println!("starting the worker: {command}");
                let child = tokio::process::Command::new("sh").arg("-c").arg(&command).env("OXILABEL_WORKER_ADDR", &addr).process_group(0).spawn();
                match child {
                    Ok(mut child) => {
                        let group = child.id().map(|id| id as i32);
                        if let Some(group) = group {
                            *this.group.lock().expect("worker group lock") = Some(group);
                            GROUPS.lock().expect("worker groups lock").insert(group);
                        }
                        let exited = child.wait().await;
                        if let Some(group) = group {
                            GROUPS.lock().expect("worker groups lock").remove(&group);
                        }
                        if this.generation.load(Ordering::SeqCst) != generation {
                            return;
                        }
                        match exited {
                            Ok(status) => eprintln!("the worker stopped ({status}); starting it again in 5 s"),
                            Err(err) => eprintln!("lost the worker: {err}"),
                        }
                    }
                    Err(err) => eprintln!("cannot start the worker: {err}; trying again in 5 s"),
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }
}
