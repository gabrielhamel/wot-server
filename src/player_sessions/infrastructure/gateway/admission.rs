use super::{
    ClientKey, Completion, Gateway, LoginError, PreparedAccount, Ticket, TracedCompletion,
};
use blowfish::Blowfish;
use rsa::rand_core::{OsRng, RngCore};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{Instrument, Span, field::Empty};

const LOGIN_TO_OTHER_GAME: u8 = 1;
const REPLACED_REASON: &str = "#system_messages:login_to_other_game_wot";

pub(super) struct LoginAttempt {
    pub addr: SocketAddr,
    pub generation: u64,
    pub account_change_epoch: u64,
    pub blowfish: Arc<Blowfish>,
}

pub(super) struct DeferredLogin {
    attempt: LoginAttempt,
    account_id: i64,
    parent: Span,
}

impl Gateway {
    pub(super) fn complete_login(
        &mut self,
        attempt: LoginAttempt,
        result: anyhow::Result<Option<PreparedAccount>>,
    ) {
        let LoginAttempt {
            addr, generation, ..
        } = attempt;
        if !self.admissions.is_pending(addr, generation, Instant::now()) {
            Span::current().record("outcome", "admission_expired");
            return;
        }
        let account = match result {
            Ok(Some(account)) => account,
            Ok(None) => {
                self.reject_login(&attempt, LoginError::InvalidPassword);
                return;
            }
            Err(error) => {
                Span::current().record("otel.status_code", "ERROR");
                tracing::error!(%error, "Account lookup failed");
                self.reject_login(&attempt, LoginError::DatabaseGeneralFailure);
                return;
            }
        };
        self.admit(attempt, account, Span::current());
    }

    #[tracing::instrument(name = "player_sessions.admit", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account.snapshot.id(), input.revision = account.snapshot.revision(), output.outcome = Empty, otel.status_code = Empty))]
    fn admit(&mut self, attempt: LoginAttempt, account: PreparedAccount, parent: Span) {
        let account_id = account.snapshot.id();
        let span = Span::current();
        let previous = self
            .sessions
            .iter()
            .find(|(_, session)| session.account.snapshot.id() == account_id)
            .map(|(&client, session)| (client, session.generation));
        let ticket = self
            .tickets
            .iter()
            .find(|(_, ticket)| ticket.account.snapshot.id() == account_id)
            .map(|(&key, ticket)| (key, ticket.generation));
        if previous.is_some_and(|(_, generation)| generation > attempt.generation)
            || ticket.is_some_and(|(_, generation)| generation > attempt.generation)
        {
            span.record("output.outcome", "superseded");
            self.reject_login(&attempt, LoginError::AlreadyLoggedIn);
            return;
        }
        if self.pending_writes.contains(&account_id) {
            span.record("output.outcome", "waiting_for_write");
            self.deferred_logins.insert(
                attempt.addr,
                DeferredLogin {
                    attempt,
                    account_id,
                    parent,
                },
            );
            return;
        }
        if attempt.account_change_epoch != self.account_change_epoch {
            span.record("output.outcome", "refreshing_snapshot");
            self.refresh_admission(attempt, account_id);
            return;
        }
        let LoginAttempt {
            addr,
            generation,
            blowfish,
            ..
        } = attempt;
        if !self.admissions.complete(addr, generation, Instant::now()) {
            span.record("output.outcome", "expired");
            return;
        }
        if previous.is_none()
            && ticket.is_none()
            && self.sessions.len() + self.tickets.len() >= self.config.max_clients
        {
            span.record("output.outcome", "full");
            self.login
                .answer_login_error(addr, LoginError::BaseAppOverload, String::new());
            return;
        }
        let destination = match self.base.addr().and_then(|bound| {
            super::super::endpoint::advertised_base(self.config.advertised_base, bound, addr)
        }) {
            Ok(destination) => destination,
            Err(error) => {
                span.record("output.outcome", "destination_unavailable");
                span.record("otel.status_code", "ERROR");
                tracing::error!(%error, "Resolve client-facing BaseApp address");
                self.login
                    .answer_login_error(addr, LoginError::NoBaseApp, String::new());
                return;
            }
        };
        let mut key = OsRng.next_u32();
        while key == 0 || self.tickets.contains_key(&key) {
            key = OsRng.next_u32();
        }
        if self
            .login
            .answer_login_success(addr, destination, key, String::new())
            .is_none()
        {
            span.record("output.outcome", "reply_unavailable");
            return;
        }
        if let Some((client, _)) = previous {
            self.replace_session(client, account_id);
        }
        if let Some((previous_key, _)) = ticket {
            self.tickets.remove(&previous_key);
        }
        self.tickets.insert(
            key,
            Ticket {
                addr,
                generation,
                blowfish,
                account,
                expires: Instant::now() + Duration::from_secs(30),
            },
        );
        span.record(
            "output.outcome",
            if previous.is_some() || ticket.is_some() {
                "replaced"
            } else {
                "admitted"
            },
        );
    }

    pub(super) fn resume_logins(&mut self) {
        let now = Instant::now();
        self.deferred_logins.retain(|&addr, login| {
            let pending = self
                .admissions
                .is_pending(addr, login.attempt.generation, now);
            if !pending {
                login.parent.record("outcome", "admission_expired");
            }
            pending
        });
        let ready: Vec<_> = self
            .deferred_logins
            .extract_if(|_, login| !self.pending_writes.contains(&login.account_id))
            .map(|(_, login)| login)
            .collect();
        for DeferredLogin {
            attempt,
            account_id,
            parent,
        } in ready
        {
            parent.in_scope(|| self.refresh_admission(attempt, account_id));
        }
    }

    fn reject_login(&mut self, attempt: &LoginAttempt, error: LoginError) {
        self.admissions
            .complete(attempt.addr, attempt.generation, Instant::now());
        self.login
            .answer_login_error(attempt.addr, error, String::new());
    }

    fn refresh_admission(&mut self, mut attempt: LoginAttempt, account_id: i64) {
        let Ok(permit) = self.workers.clone().try_acquire_owned() else {
            Span::current().record("output.outcome", "refresh_busy");
            self.reject_login(&attempt, LoginError::RateLimited);
            return;
        };
        attempt.account_change_epoch = self.account_change_epoch;
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(
            async move {
                let _permit = permit;
                let result = async {
                    let snapshot = accounts.snapshot(account_id).await?;
                    tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
                        move || catalog.prepare(snapshot),
                    ))
                    .await?
                    .map(Some)
                }
                .await;
                if sender
                    .try_send(TracedCompletion::new(Completion::Login { attempt, result }))
                    .is_err()
                {
                    tracing::error!("Login snapshot refresh completion queue unavailable");
                }
            }
            .in_current_span(),
        );
    }

    #[tracing::instrument(name = "player_sessions.replace_session", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account_id, output.delivery = Empty, otel.status_code = Empty))]
    fn replace_session(&mut self, client: ClientKey, account_id: i64) {
        let span = Span::current();
        if let Some(entity) = self
            .sessions
            .get(&client)
            .and_then(|session| session.entity)
        {
            let result = self
                .base
                .entity(entity)
                .and_then(|mut account| {
                    account.onKickedFromServer(REPLACED_REASON.to_owned(), LOGIN_TO_OTHER_GAME, 0)
                })
                .and_then(|()| self.base.flush_client(client));
            match result {
                Ok(()) => {
                    span.record("output.delivery", "sent");
                }
                Err(error) => {
                    span.record("output.delivery", "failed");
                    span.record("otel.status_code", "ERROR");
                    tracing::warn!(%error, "Could not send replacement notice");
                }
            }
        } else {
            span.record("output.delivery", "unavailable");
        }
        self.base.disconnect_client(client);
        self.remove_session(client);
    }

    pub(super) fn remove_session(&mut self, client: ClientKey) {
        if let Some(session) = self.sessions.get(&client) {
            self.xmpp.revoke(session.account.snapshot.id());
            self.social_disconnect(client);
            self.remove_training_member(client);
            for room in self.training_rooms.values_mut() {
                room.remove_invite(client);
            }
            self.sessions.remove(&client);
        }
    }
}
