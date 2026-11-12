//! Background worker: sends queued scrobbles, one request per second at most.
//!
//! One thread owns the Last.fm client. The UI talks to it through [`Scrobbler`]
//! (a channel) and reads [`Status`], so no Last.fm call ever runs on the UI thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::{queue, Api, ApiError, Client, Item, Meta};

/// Minimum time between two requests to Last.fm.
const REQUEST_GAP: Duration = Duration::from_secs(1);
/// How long a user has to allow Abyss on last.fm.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(180);
const CONNECT_POLL: Duration = Duration::from_secs(3);
const BACKOFF_FIRST: Duration = Duration::from_secs(60);
const BACKOFF_MAX: Duration = Duration::from_secs(900);

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// Waiting for the user to allow Abyss on last.fm.
    Connecting,
    /// Sending the saved history.
    Syncing,
}

/// What the UI shows. `version` changes on every update.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub version: u64,
    pub phase: Phase,
    /// Listens waiting to be sent.
    pub queued: usize,
    pub total: usize,
    pub done: usize,
    pub error: Option<String>,
    /// A session the UI must store (user name, key). Taken once.
    pub new_session: Option<(String, String)>,
    /// The stored session was rejected: the UI must forget it.
    pub session_invalid: bool,
    /// A history sync finished.
    pub synced: bool,
}

/// Spaces requests at least `gap` apart.
pub struct Limiter {
    next: Option<Instant>,
    gap: Duration,
}

impl Limiter {
    pub fn new(gap: Duration) -> Self {
        Self { next: None, gap }
    }

    pub fn wait_time(&self, now: Instant) -> Duration {
        self.next.map_or(Duration::ZERO, |next| next.saturating_duration_since(now))
    }

    /// Record a request made at `now`.
    pub fn book(&mut self, now: Instant) {
        self.next = Some(now.max(self.next.unwrap_or(now)) + self.gap);
    }
}

#[derive(Debug, PartialEq)]
pub enum Step {
    /// Nothing to do until a job arrives.
    Idle,
    /// A batch was handled; there may be more.
    Sent,
    /// Try again after this long.
    Wait(Duration),
}

pub struct Engine<A: Api> {
    api: A,
    dir: PathBuf,
    session: Option<String>,
    enabled: bool,
    /// Stopped by an error that only new settings can fix.
    halted: bool,
    limiter: Limiter,
    failures: u32,
    retry_at: Option<Instant>,
    status: Arc<Mutex<Status>>,
    open: fn(&str),
    poll: Duration,
}

impl<A: Api> Engine<A> {
    pub fn new(api: A, dir: PathBuf, status: Arc<Mutex<Status>>) -> Self {
        Self {
            api,
            dir,
            session: None,
            enabled: false,
            halted: false,
            limiter: Limiter::new(REQUEST_GAP),
            failures: 0,
            retry_at: None,
            status,
            open: |url| {
                let _ = open::that(url);
            },
            poll: CONNECT_POLL,
        }
    }

    fn update(&self, change: impl FnOnce(&mut Status)) {
        let mut status = self.status.lock();
        change(&mut status);
        status.version += 1;
    }

    fn ready(&self) -> bool {
        self.enabled && !self.halted && self.session.is_some()
    }

    fn refresh_queued(&self) {
        let queued = queue::pending(&self.dir).unwrap_or(0);
        self.update(|s| s.queued = queued);
    }

    pub fn configure(&mut self, session: Option<String>, enabled: bool) {
        self.session = session.filter(|s| !s.is_empty());
        self.enabled = enabled;
        self.halted = false;
        self.failures = 0;
        self.retry_at = None;
        self.update(|s| s.error = None);
        self.refresh_queued();
    }

    /// Queue a finished listen.
    pub fn push(&mut self, item: &Item) {
        if !self.ready() {
            return;
        }
        match queue::push(&self.dir, item) {
            Ok(_) => self.refresh_queued(),
            Err(error) => log::warn!("Last.fm queue: {error:#}"),
        }
    }

    pub fn limiter_wait(&self, now: Instant) -> Duration {
        self.limiter.wait_time(now)
    }

    /// Best effort: "now playing" is never queued or retried.
    pub fn send_now_playing(&mut self, meta: &Meta, now: Instant) {
        let Some(session) = self.session.clone().filter(|_| self.ready()) else { return };
        self.limiter.book(now);
        match self.api.now_playing(&session, meta) {
            Ok(()) => {}
            Err(ApiError::InvalidSession) => self.session_rejected(),
            Err(error) => log::debug!("Last.fm now playing: {error}"),
        }
    }

    fn session_rejected(&mut self) {
        self.session = None;
        self.update(|s| {
            s.session_invalid = true;
            s.phase = Phase::Idle;
        });
    }

    /// Queue the saved Replay history and start sending it.
    pub fn start_backfill(&mut self) {
        if !self.ready() {
            return;
        }
        match queue::enqueue_history(&self.dir) {
            Ok(_) => {
                let total = queue::pending(&self.dir).unwrap_or(0);
                self.update(|s| {
                    s.phase = Phase::Syncing;
                    s.total = total;
                    s.done = 0;
                    s.queued = total;
                    s.error = None;
                    s.synced = false;
                });
            }
            Err(error) => {
                log::warn!("Last.fm history: {error:#}");
                self.update(|s| s.error = Some(format!("{error:#}")));
            }
        }
    }

    /// Stop a history sync: what has not been sent yet is dropped from the queue.
    pub fn cancel_backfill(&mut self) {
        if let Err(error) = queue::discard_replay(&self.dir) {
            log::warn!("Last.fm cancel: {error:#}");
        }
        self.update(|s| s.phase = Phase::Idle);
        self.refresh_queued();
    }

    /// Send at most one batch.
    pub fn step(&mut self, now: Instant) -> Step {
        let Some(session) = self.session.clone().filter(|_| self.ready()) else { return Step::Idle };
        if let Some(at) = self.retry_at {
            if now < at {
                return Step::Wait(at - now);
            }
            self.retry_at = None;
        }
        let wait = self.limiter.wait_time(now);
        if !wait.is_zero() {
            return Step::Wait(wait);
        }
        let batch = match queue::take(&self.dir, queue::BATCH) {
            Ok(batch) => batch,
            Err(error) => {
                log::warn!("Last.fm queue: {error:#}");
                self.retry_at = Some(now + BACKOFF_FIRST);
                return Step::Wait(BACKOFF_FIRST);
            }
        };
        if batch.is_empty() {
            self.update(|s| {
                if s.phase == Phase::Syncing {
                    s.phase = Phase::Idle;
                    s.synced = true;
                }
                s.queued = 0;
            });
            return Step::Idle;
        }
        self.limiter.book(now);
        match self.api.scrobble(&session, &batch) {
            Ok(sent) => {
                if sent.ignored > 0 {
                    log::info!("Last.fm ignored {} of {} scrobbles", sent.ignored, batch.len());
                }
                self.finish(&batch, true);
                self.failures = 0;
            }
            Err(ApiError::InvalidSession) => {
                self.session_rejected();
                return Step::Idle;
            }
            Err(ApiError::Retry(message)) => {
                self.failures += 1;
                let delay = BACKOFF_FIRST.saturating_mul(1 << (self.failures - 1).min(8)).min(BACKOFF_MAX);
                self.retry_at = Some(now + delay);
                self.update(|s| s.error = Some(message));
                return Step::Wait(delay);
            }
            Err(ApiError::Fatal(message)) => {
                self.halted = true;
                self.update(|s| {
                    s.error = Some(message);
                    s.phase = Phase::Idle;
                });
                return Step::Idle;
            }
            Err(error) => {
                // Pending or Permanent: this batch can never be accepted. Drop it so
                // it cannot block the listens behind it.
                log::warn!("Last.fm rejected {} scrobbles: {error}", batch.len());
                self.finish(&batch, false);
            }
        }
        Step::Sent
    }

    fn finish(&mut self, batch: &[Item], sent: bool) {
        let result = if sent { queue::complete(&self.dir, batch) } else { queue::discard(&self.dir, batch) };
        if let Err(error) = result {
            log::warn!("Last.fm queue: {error:#}");
        }
        let queued = queue::pending(&self.dir).unwrap_or(0);
        self.update(|s| {
            s.done += batch.len();
            s.queued = queued;
            s.error = None;
        });
    }

    /// Ask the user to allow Abyss on last.fm and wait for it.
    fn connect(&mut self, jobs: &Receiver<Job>) {
        self.update(|s| {
            s.phase = Phase::Connecting;
            s.error = None;
        });
        let fail = |this: &Self, message: String| {
            this.update(|s| {
                s.phase = Phase::Idle;
                s.error = Some(message);
            })
        };
        let token = match self.api.auth_token() {
            Ok(token) => token,
            Err(error) => return fail(self, error.to_string()),
        };
        (self.open)(&self.api.auth_url(&token));
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            match jobs.recv_timeout(self.poll) {
                Ok(Job::Cancel) => return self.update(|s| s.phase = Phase::Idle),
                Err(RecvTimeoutError::Disconnected) => return,
                _ => {}
            }
            match self.api.session(&token) {
                Ok((name, key)) => {
                    if let Err(error) = queue::set_account(&self.dir, &name) {
                        log::warn!("Last.fm account: {error:#}");
                    }
                    return self.update(|s| {
                        s.phase = Phase::Idle;
                        s.new_session = Some((name, key));
                    });
                }
                Err(ApiError::Pending) if Instant::now() < deadline => {}
                Err(ApiError::Pending) => return fail(self, "timed out".into()),
                Err(error) => return fail(self, error.to_string()),
            }
        }
    }

    fn handle(&mut self, job: Job, jobs: &Receiver<Job>) {
        match job {
            Job::Configure { session, enabled } => self.configure(session, enabled),
            Job::NowPlaying(meta) => {
                let wait = self.limiter_wait(Instant::now());
                std::thread::sleep(wait);
                self.send_now_playing(&meta, Instant::now());
            }
            Job::Scrobble(item) => self.push(&item),
            Job::Connect => self.connect(jobs),
            Job::Sync => self.start_backfill(),
            Job::Cancel => self.cancel_backfill(),
        }
    }
}

pub enum Job {
    Configure { session: Option<String>, enabled: bool },
    NowPlaying(Meta),
    Scrobble(Item),
    Connect,
    Sync,
    Cancel,
}

/// The worker loop. Ends when the sender is dropped.
pub fn run<A: Api>(mut engine: Engine<A>, jobs: Receiver<Job>) {
    loop {
        loop {
            match jobs.try_recv() {
                Ok(job) => engine.handle(job, &jobs),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match engine.step(Instant::now()) {
            Step::Sent => {}
            Step::Wait(duration) => match jobs.recv_timeout(duration) {
                Ok(job) => engine.handle(job, &jobs),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            },
            Step::Idle => match jobs.recv() {
                Ok(job) => engine.handle(job, &jobs),
                Err(_) => return,
            },
        }
    }
}

/// Handle owned by the app: sends jobs to the worker thread and reads its status.
pub struct Scrobbler {
    tx: Sender<Job>,
    pub status: Arc<Mutex<Status>>,
}

impl Scrobbler {
    pub fn spawn(dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let spawned = std::thread::Builder::new().name("scrobbler".into()).spawn(move || run(Engine::new(Client::new(), dir, shared), rx));
        if let Err(error) = spawned {
            log::warn!("Last.fm worker did not start: {error}");
        }
        Self { tx, status }
    }

    fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    /// Apply the stored session and the on/off setting.
    pub fn configure(&self, session: Option<String>, enabled: bool) {
        self.send(Job::Configure { session, enabled });
    }

    pub fn now_playing(&self, meta: Meta) {
        self.send(Job::NowPlaying(meta));
    }

    pub fn scrobble(&self, item: Item) {
        self.send(Job::Scrobble(item));
    }

    pub fn connect(&self) {
        self.send(Job::Connect);
    }

    pub fn sync_history(&self) {
        self.send(Job::Sync);
    }

    pub fn cancel(&self) {
        self.send(Job::Cancel);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;
    use crate::scrobble::Sent;

    /// Replays scripted answers and records every call.
    #[derive(Default)]
    struct Fake {
        scrobbles: RefCell<Vec<usize>>,
        script: RefCell<VecDeque<Result<Sent, ApiError>>>,
        sessions: RefCell<VecDeque<Result<(String, String), ApiError>>>,
        now_playing: RefCell<Vec<String>>,
    }

    impl Api for &Fake {
        fn auth_token(&self) -> Result<String, ApiError> {
            Ok("tok".into())
        }
        fn auth_url(&self, token: &str) -> String {
            format!("https://example.test/{token}")
        }
        fn session(&self, _: &str) -> Result<(String, String), ApiError> {
            self.sessions.borrow_mut().pop_front().unwrap_or(Err(ApiError::Pending))
        }
        fn now_playing(&self, _: &str, meta: &Meta) -> Result<(), ApiError> {
            self.now_playing.borrow_mut().push(meta.track.clone());
            Ok(())
        }
        fn scrobble(&self, _: &str, items: &[Item]) -> Result<Sent, ApiError> {
            self.scrobbles.borrow_mut().push(items.len());
            self.script.borrow_mut().pop_front().unwrap_or(Ok(Sent { accepted: items.len() as u32, ignored: 0 }))
        }
    }

    fn item(n: u64) -> Item {
        Item {
            id: format!("i{n:04}"),
            ts: n,
            meta: Meta { artist: "A".into(), track: format!("T{n}"), album: String::new(), duration: 200 },
        }
    }

    fn engine<'a>(fake: &'a Fake, dir: &std::path::Path) -> Engine<&'a Fake> {
        let mut engine = Engine::new(fake, dir.to_path_buf(), Arc::default());
        engine.open = |_| {};
        engine.poll = Duration::from_millis(5);
        engine.configure(Some("sk".into()), true);
        engine
    }

    #[test]
    fn limiter_spaces_requests_one_second_apart() {
        let t0 = Instant::now();
        let mut limiter = Limiter::new(Duration::from_secs(1));
        assert_eq!(limiter.wait_time(t0), Duration::ZERO);
        limiter.book(t0);
        assert_eq!(limiter.wait_time(t0), Duration::from_secs(1));
        assert_eq!(limiter.wait_time(t0 + Duration::from_millis(400)), Duration::from_millis(600));
        assert_eq!(limiter.wait_time(t0 + Duration::from_secs(2)), Duration::ZERO);
    }

    #[test]
    fn sends_in_batches_of_fifty_one_request_per_second() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let mut engine = engine(&fake, dir.path());
        for n in 0..120 {
            engine.push(&item(n));
        }
        let t0 = Instant::now();
        assert_eq!(engine.step(t0), Step::Sent);
        assert!(matches!(engine.step(t0), Step::Wait(d) if d == Duration::from_secs(1)), "the next request waits a second");
        assert_eq!(engine.step(t0 + Duration::from_secs(1)), Step::Sent);
        assert_eq!(engine.step(t0 + Duration::from_secs(2)), Step::Sent);
        assert_eq!(engine.step(t0 + Duration::from_secs(3)), Step::Idle);
        assert_eq!(*fake.scrobbles.borrow(), [50, 50, 20]);
        assert_eq!(queue::pending(dir.path()).unwrap(), 0);
    }

    #[test]
    fn a_temporary_failure_keeps_the_listens_and_backs_off() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        fake.script.borrow_mut().push_back(Err(ApiError::Retry("offline".into())));
        let mut engine = engine(&fake, dir.path());
        engine.push(&item(1));
        let t0 = Instant::now();
        assert_eq!(engine.step(t0), Step::Wait(BACKOFF_FIRST));
        assert_eq!(queue::pending(dir.path()).unwrap(), 1, "nothing is lost");
        assert!(matches!(engine.step(t0 + Duration::from_secs(30)), Step::Wait(_)));
        assert_eq!(fake.scrobbles.borrow().len(), 1, "no request during the backoff");
        assert_eq!(engine.step(t0 + BACKOFF_FIRST), Step::Sent);
        assert_eq!(queue::pending(dir.path()).unwrap(), 0);
        assert_eq!(engine.status.lock().error, None);
    }

    #[test]
    fn backoff_doubles_and_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        for _ in 0..8 {
            fake.script.borrow_mut().push_back(Err(ApiError::Retry("down".into())));
        }
        let mut engine = engine(&fake, dir.path());
        engine.push(&item(1));
        let mut now = Instant::now();
        let mut delays = Vec::new();
        for _ in 0..6 {
            if let Step::Wait(delay) = engine.step(now) {
                delays.push(delay.as_secs());
                now += delay;
            }
        }
        assert_eq!(delays, [60, 120, 240, 480, 900, 900]);
    }

    #[test]
    fn an_invalid_session_stops_and_tells_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        fake.script.borrow_mut().push_back(Err(ApiError::InvalidSession));
        let mut engine = engine(&fake, dir.path());
        engine.push(&item(1));
        assert_eq!(engine.step(Instant::now()), Step::Idle);
        assert!(engine.status.lock().session_invalid);
        assert_eq!(queue::pending(dir.path()).unwrap(), 1, "kept for when the user connects again");
        assert_eq!(engine.step(Instant::now() + Duration::from_secs(5)), Step::Idle);
        assert_eq!(fake.scrobbles.borrow().len(), 1);
    }

    #[test]
    fn a_rejected_batch_is_dropped_so_it_cannot_block_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        fake.script.borrow_mut().push_back(Err(ApiError::Permanent(6, "bad".into())));
        let mut engine = engine(&fake, dir.path());
        engine.push(&item(1));
        assert_eq!(engine.step(Instant::now()), Step::Sent);
        assert_eq!(queue::pending(dir.path()).unwrap(), 0);
        // It was not marked as sent, so a later backfill could try it again.
        assert!(queue::push(dir.path(), &item(1)).unwrap());
    }

    #[test]
    fn an_invalid_application_key_halts_until_reconfigured() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        fake.script.borrow_mut().push_back(Err(ApiError::Fatal("Invalid API key".into())));
        let mut engine = engine(&fake, dir.path());
        engine.push(&item(1));
        assert_eq!(engine.step(Instant::now()), Step::Idle);
        assert_eq!(engine.status.lock().error.as_deref(), Some("Invalid API key"));
        assert_eq!(engine.step(Instant::now() + Duration::from_secs(5)), Step::Idle);
        assert_eq!(fake.scrobbles.borrow().len(), 1);
    }

    #[test]
    fn nothing_is_queued_or_sent_while_disconnected_or_off() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let mut engine = engine(&fake, dir.path());
        engine.configure(None, true);
        engine.push(&item(1));
        engine.configure(Some("sk".into()), false);
        engine.push(&item(2));
        assert_eq!(queue::pending(dir.path()).unwrap(), 0);
        assert_eq!(engine.step(Instant::now()), Step::Idle);
        engine.send_now_playing(&item(3).meta, Instant::now());
        assert!(fake.now_playing.borrow().is_empty());
    }

    #[test]
    fn history_sync_reports_progress_and_finishes() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::transaction(dir.path(), |tx| {
            tx.execute_batch("CREATE TABLE history (recording TEXT PRIMARY KEY NOT NULL, started INTEGER NOT NULL, seconds INTEGER NOT NULL, entry TEXT NOT NULL)")?;
            for n in 0..60u64 {
                let entry = crate::history::Entry {
                    ts: 1_000 + n * 1_000,
                    secs: 150,
                    id: n,
                    title: format!("Song {n}"),
                    artist: "Artist".into(),
                    artist_id: 1,
                    album: "Album".into(),
                    album_id: 1,
                    cover: None,
                    dur: 200,
                };
                tx.execute(
                    "INSERT INTO history VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![format!("r{n}"), entry.ts, entry.secs, serde_json::to_string(&entry)?],
                )?;
            }
            Ok(())
        })
        .unwrap();
        let fake = Fake::default();
        let mut engine = engine(&fake, dir.path());
        engine.start_backfill();
        {
            let status = engine.status.lock();
            assert_eq!((status.phase, status.total, status.done), (Phase::Syncing, 60, 0));
        }
        let t0 = Instant::now();
        assert_eq!(engine.step(t0), Step::Sent);
        assert_eq!(engine.status.lock().done, 50);
        assert_eq!(engine.step(t0 + Duration::from_secs(1)), Step::Sent);
        assert_eq!(engine.step(t0 + Duration::from_secs(2)), Step::Idle);
        let status = engine.status.lock();
        assert_eq!((status.phase, status.done, status.synced), (Phase::Idle, 60, true));
        drop(status);
        // A second sync finds nothing new.
        engine.start_backfill();
        assert_eq!(engine.status.lock().total, 0);
    }

    #[test]
    fn cancelling_a_sync_drops_only_the_history_listens() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let mut engine = engine(&fake, dir.path());
        engine.push(&item(1));
        queue::push(dir.path(), &Item { id: "replay:x".into(), ..item(2) }).unwrap();
        engine.cancel_backfill();
        let ids: Vec<String> = queue::take(dir.path(), 10).unwrap().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["i0001"]);
    }

    #[test]
    fn connecting_waits_for_the_user_then_hands_over_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        fake.sessions.borrow_mut().extend([
            Err(ApiError::Pending),
            Err(ApiError::Pending),
            Ok(("Rauna".to_string(), "sk-new".to_string())),
        ]);
        let mut engine = engine(&fake, dir.path());
        let (_tx, rx) = mpsc::channel();
        engine.connect(&rx);
        let status = engine.status.lock();
        assert_eq!(status.new_session, Some(("Rauna".to_string(), "sk-new".to_string())));
        assert_eq!(status.phase, Phase::Idle);
    }

    #[test]
    fn connecting_can_be_cancelled_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let mut engine = engine(&fake, dir.path());
        let (tx, rx) = mpsc::channel();
        tx.send(Job::Cancel).unwrap();
        engine.connect(&rx);
        assert_eq!(engine.status.lock().new_session, None);
        assert_eq!(engine.status.lock().phase, Phase::Idle);

        fake.sessions.borrow_mut().push_back(Err(ApiError::Permanent(15, "Token expired".into())));
        let (_tx2, rx2) = mpsc::channel();
        engine.connect(&rx2);
        assert!(engine.status.lock().error.as_deref().is_some_and(|e| e.contains("Token expired")));
    }
}
