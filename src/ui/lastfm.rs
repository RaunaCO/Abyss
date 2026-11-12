//! Last.fm: connecting an account, scrobbling hooks and sync. The settings
//! rows are in `settings.rs`; the network work runs in `crate::scrobble`.
use gpui::Context;

use super::AppView;
use crate::scrobble::{tracker::Event, worker::Phase};

impl AppView {
    /// Give the worker the stored session and the on/off setting.
    pub fn lastfm_apply(&self) {
        let s = self.settings.lock();
        self.scrobbler.configure(s.lastfm_session.clone(), s.lastfm_enabled);
    }

    pub fn lastfm_connected(&self) -> bool {
        self.settings.lock().lastfm_session.is_some()
    }

    /// Called every tick: feeds the listen tracker and picks up what the worker reports.
    /// Returns true when the screen should be redrawn.
    pub fn lastfm_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let dt = self.lastfm_tick.elapsed().as_secs_f32();
        self.lastfm_tick = std::time::Instant::now();
        let active = {
            let s = self.settings.lock();
            s.lastfm_enabled && s.lastfm_session.is_some()
        };
        if active {
            let track = self.now.as_ref().filter(|_| !self.ended);
            let audible = track.is_some_and(|n| Some(n.key) == self.snap.key) && !self.snap.paused && !self.snap.buffering;
            let events = self.scrobble_tracker.tick(track.map(|n| (n.key, &n.track)), audible, dt, crate::history::unix_now());
            for event in events {
                match event {
                    Event::NowPlaying(meta) => self.scrobbler.now_playing(meta),
                    Event::Scrobble(item) => self.scrobbler.scrobble(item),
                }
            }
        }
        // What the worker reports: a new session to store, a rejected one, progress.
        let (version, new_session, invalid, synced) = {
            let mut status = self.scrobbler.status.lock();
            if status.version == self.lastfm_seen {
                return false;
            }
            (status.version, status.new_session.take(), std::mem::take(&mut status.session_invalid), std::mem::take(&mut status.synced))
        };
        self.lastfm_seen = version;
        if let Some((user, key)) = new_session {
            {
                let mut s = self.settings.lock();
                s.lastfm_user = Some(user);
                s.lastfm_session = Some(key);
                s.lastfm_enabled = true;
                s.save();
            }
            self.lastfm_apply();
        }
        if invalid {
            {
                let mut s = self.settings.lock();
                s.lastfm_session = None;
                s.lastfm_enabled = false;
                s.save();
            }
            self.lastfm_apply();
            self.notify_toast(crate::i18n::t("lastfm.session_expired"), cx);
        }
        if synced {
            self.notify_toast(crate::i18n::t("lastfm.sync_done"), cx);
        }
        true
    }

    pub fn lastfm_connect(&mut self, cx: &mut Context<Self>) {
        if !crate::scrobble::configured() {
            self.notify_toast(crate::i18n::t("lastfm.not_available"), cx);
            return;
        }
        self.scrobbler.connect();
        cx.notify();
    }

    pub fn lastfm_disconnect(&mut self, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            s.lastfm_session = None;
            s.lastfm_user = None;
            s.lastfm_enabled = false;
            s.save();
        }
        self.lastfm_apply();
        cx.notify();
    }

    pub fn lastfm_toggle(&mut self, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            if s.lastfm_session.is_none() {
                return;
            }
            s.lastfm_enabled = !s.lastfm_enabled;
            s.save();
        }
        self.lastfm_apply();
        cx.notify();
    }

    pub fn lastfm_sync(&mut self, cx: &mut Context<Self>) {
        if self.lastfm_connected() && !self.settings.lock().lastfm_enabled {
            // A sync needs sending to be on.
            self.lastfm_toggle(cx);
        }
        self.scrobbler.sync_history();
        cx.notify();
    }

    pub fn lastfm_cancel(&mut self, cx: &mut Context<Self>) {
        self.scrobbler.cancel();
        cx.notify();
    }

    /// The line under the Last.fm title: who is connected, or what is happening.
    pub fn lastfm_status_text(&self) -> String {
        let status = self.scrobbler.status.lock().clone();
        let user = self.settings.lock().lastfm_user.clone();
        match status.phase {
            Phase::Connecting => crate::i18n::t("lastfm.status_connecting").to_string(),
            Phase::Syncing => {
                crate::i18n::text("lastfm.status_syncing", &[status.done.min(status.total).to_string(), status.total.to_string()])
            }
            Phase::Idle => match (&status.error, user) {
                (Some(error), _) => crate::i18n::text("lastfm.status_error", std::slice::from_ref(error)),
                (None, Some(user)) => crate::i18n::text("lastfm.status_on", &[user]),
                (None, None) => crate::i18n::t("lastfm.status_off").to_string(),
            },
        }
    }

    pub fn lastfm_phase(&self) -> Phase {
        self.scrobbler.status.lock().phase
    }
}
