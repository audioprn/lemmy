//! audioprn: how many creators, members and guests are online (the top bar's line). Each open
//! tab calls GET /audioprn/online every minute; whoever called in the past 15 minutes is online.
//! Kept in memory only (guests by IP address), so a restart starts from zero.

use crate::is_verified_creator;
use activitypub_federation::config::Data;
use actix_web::{HttpRequest, web::Json};
use lemmy_api_utils::context::LemmyContext;
use lemmy_db_views_local_user::LocalUserView;
use lemmy_utils::error::LemmyResult;
use serde::Serialize;
use std::{
  collections::HashMap,
  sync::{LazyLock, Mutex},
  time::{Duration, Instant},
};

const ONLINE_FOR: Duration = Duration::from_secs(15 * 60);
/// Counting goes through everyone, so it's done at most once a minute (or for a new visitor).
const RECOUNT_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Default, Serialize)]
pub struct Online {
  /// Verified creators.
  pub creators: usize,
  /// Other registered users.
  pub members: usize,
  pub guests: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
  Creator,
  Member,
  Guest,
}

#[derive(Default)]
struct Seen {
  last: HashMap<String, (Instant, Kind)>,
  counted_at: Option<Instant>,
  counts: Online,
}

static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(Default::default);

/// GET /audioprn/online
pub async fn get_online(
  req: HttpRequest,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<Online>> {
  let (key, kind) = match local_user_view {
    Some(u) => {
      let creator = is_verified_creator(&mut context.pool(), u.person.id).await?;
      let kind = if creator { Kind::Creator } else { Kind::Member };
      (format!("p{}", u.person.id.0), kind)
    }
    None => {
      let ip = req.connection_info().realip_remote_addr().unwrap_or("").to_string();
      (format!("ip{ip}"), Kind::Guest)
    }
  };
  let now = Instant::now();
  let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
  let new = seen.last.insert(key, (now, kind)).is_none();
  if new || seen.counted_at.is_none_or(|t| now - t > RECOUNT_EVERY) {
    seen.last.retain(|_, (t, _)| now - *t < ONLINE_FOR);
    let count = |k| seen.last.values().filter(|(_, kind)| *kind == k).count();
    let counts = Online {
      creators: count(Kind::Creator),
      members: count(Kind::Member),
      guests: count(Kind::Guest),
    };
    seen.counts = counts;
    seen.counted_at = Some(now);
  }
  Ok(Json(seen.counts))
}
