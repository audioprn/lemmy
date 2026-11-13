//! audioprn "My audioprn": each user's feed preferences (the feed itself is the
//! `audioprn_my_feed` SQL function, used by the post list's `my_recent` / `my_feed` filters),
//! and creators removing comments on their own profile posts.

use activitypub_federation::config::Data;
use actix_web::web::Json;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, SelectableHelper, dsl::insert_into};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_schema_file::schema::{comment, community, post};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

diesel::table! {
  feed_prefs (person_id) {
    person_id -> Int4,
    f_listener -> Bool,
    m_listener -> Bool,
    f_voices -> Bool,
    m_voices -> Bool,
    scripts -> Bool,
    audios -> Bool,
    favourite_tags -> Array<Text>,
    excluded_tags -> Array<Text>,
  }
}

const MAX_FAVOURITE_TAGS: usize = 10;
const MAX_EXCLUDED_TAGS: usize = 50;
const MAX_TAG_LENGTH: usize = 40;

#[derive(
  Debug, Serialize, Deserialize, diesel::Queryable, diesel::Selectable, diesel::Insertable, diesel::AsChangeset,
)]
#[diesel(table_name = feed_prefs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct FeedPrefs {
  pub f_listener: bool,
  pub m_listener: bool,
  pub f_voices: bool,
  pub m_voices: bool,
  pub scripts: bool,
  pub audios: bool,
  /// Bracket tags without brackets, upper case, as stored for posts.
  pub favourite_tags: Vec<String>,
  pub excluded_tags: Vec<String>,
}

impl Default for FeedPrefs {
  fn default() -> Self {
    FeedPrefs {
      f_listener: true,
      m_listener: true,
      f_voices: false,
      m_voices: false,
      scripts: false,
      audios: false,
      favourite_tags: vec![],
      excluded_tags: vec![],
    }
  }
}

/// "[cnc], Gore " -> ["CNC", "GORE"]: no brackets, upper case, no blanks or repeats.
fn clean_tags(tags: Vec<String>, max: usize) -> LemmyResult<Vec<String>> {
  let mut out: Vec<String> = vec![];
  for tag in tags {
    let tag = tag.trim().trim_start_matches('[').trim_end_matches(']').trim().to_uppercase();
    if tag.is_empty() || out.contains(&tag) {
      continue;
    }
    if tag.chars().count() > MAX_TAG_LENGTH || tag.contains(['[', ']']) {
      return Err(LemmyErrorType::Unknown("invalid_feed_tag".into()).into());
    }
    out.push(tag);
  }
  if out.len() > max {
    return Err(LemmyErrorType::Unknown("too_many_feed_tags".into()).into());
  }
  Ok(out)
}

/// GET /feed_prefs: the user's My audioprn preferences (the defaults if never saved).
pub async fn get_feed_prefs(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<FeedPrefs>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let prefs = feed_prefs::table
    .find(local_user_view.person.id.0)
    .select(FeedPrefs::as_select())
    .first(conn)
    .await
    .optional()?;
  Ok(Json(prefs.unwrap_or_default()))
}

/// PUT /feed_prefs: saves the user's preferences (at most 10 favourite tags).
pub async fn set_feed_prefs(
  Json(data): Json<FeedPrefs>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<FeedPrefs>> {
  let prefs = FeedPrefs {
    favourite_tags: clean_tags(data.favourite_tags, MAX_FAVOURITE_TAGS)?,
    excluded_tags: clean_tags(data.excluded_tags, MAX_EXCLUDED_TAGS)?,
    ..data
  };
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let person_id = feed_prefs::person_id.eq(local_user_view.person.id.0);
  insert_into(feed_prefs::table)
    .values((person_id, &prefs))
    .on_conflict(feed_prefs::person_id)
    .do_update()
    .set(&prefs)
    .execute(conn)
    .await?;
  Ok(Json(prefs))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveProfileComment {
  pub comment_id: i32,
  pub removed: bool,
}

/// PUT /profile_comment/remove: a creator removes a comment on one of their own profile posts
/// (the `creators` community; only admins restore one). Comments on Content posts go through a report instead.
pub async fn remove_profile_comment(
  Json(data): Json<RemoveProfileComment>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<RemoveProfileComment>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let (post_creator_id, community_name): (i32, String) = comment::table
    .inner_join(post::table.inner_join(community::table))
    .filter(comment::id.eq(data.comment_id))
    .select((post::creator_id, community::name))
    .first(conn)
    .await
    .optional()?
    .ok_or(LemmyErrorType::NotFound)?;
  // Removing only: a creator can't bring back a comment (an admin may have removed it).
  if !data.removed
    || post_creator_id != local_user_view.person.id.0
    || community_name != "creators"
  {
    return Err(LemmyErrorType::NotAModOrAdmin.into());
  }
  diesel::update(comment::table.find(data.comment_id))
    .set(comment::removed.eq(data.removed))
    .execute(conn)
    .await?;
  Ok(Json(data))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tags_are_cleaned() {
    let tags = vec!["[cnc]".into(), " Gore ".into(), "".into(), "CNC".into()];
    assert_eq!(clean_tags(tags, 10).ok(), Some(vec!["CNC".into(), "GORE".into()]));
    assert!(clean_tags(vec!["a]b".into()], 10).is_err());
    assert!(clean_tags((0..11).map(|i| i.to_string()).collect(), 10).is_err());
  }
}
