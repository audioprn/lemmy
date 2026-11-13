//! audioprn polls on creators' page posts (the `creators` community). The post's creator adds one
//! poll with 2-6 options and an end date. One choice per vote, and a vote can't be changed.
//! Results show after you vote, once the poll has ended, and always to the post's creator.

use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use chrono::{DateTime, Utc};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, dsl::insert_into};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_schema_file::schema::{community, post};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

diesel::table! {
  post_poll (post_id) {
    post_id -> Int4,
    question -> Text,
    options -> Array<Text>,
    ends_at -> Timestamptz,
  }
}

diesel::table! {
  post_poll_vote (post_id, person_id) {
    post_id -> Int4,
    person_id -> Int4,
    choice -> Int4,
  }
}

const MAX_OPTIONS: usize = 6;
const MAX_QUESTION_LENGTH: usize = 200;
const MAX_OPTION_LENGTH: usize = 100;
const MAX_POST_IDS: usize = 100;

#[derive(Debug, Serialize)]
pub struct PostPollView {
  pub post_id: i32,
  pub question: String,
  pub options: Vec<String>,
  pub ends_at: DateTime<Utc>,
  /// The option the logged-in user chose.
  pub my_vote: Option<i32>,
  /// Votes per option; only after voting, once ended, or for the post's creator.
  pub counts: Option<Vec<i64>>,
}

async fn load(
  conn: &mut AsyncPgConnection,
  ids: &[i32],
  me: Option<i32>,
) -> LemmyResult<Vec<PostPollView>> {
  // No joins with Lemmy's tables (see the note in lib.rs): post creators load separately.
  let polls: Vec<(i32, String, Vec<String>, DateTime<Utc>)> = post_poll::table
    .filter(post_poll::post_id.eq_any(ids))
    .select((
      post_poll::post_id,
      post_poll::question,
      post_poll::options,
      post_poll::ends_at,
    ))
    .load(conn)
    .await?;
  let creators: HashMap<i32, i32> = post::table
    .filter(post::id.eq_any(polls.iter().map(|p| p.0).collect::<Vec<_>>()))
    .select((post::id, post::creator_id))
    .load::<(i32, i32)>(conn)
    .await?
    .into_iter()
    .collect();
  let counts: Vec<(i32, i32, i64)> = post_poll_vote::table
    .filter(post_poll_vote::post_id.eq_any(ids))
    .group_by((post_poll_vote::post_id, post_poll_vote::choice))
    .select((
      post_poll_vote::post_id,
      post_poll_vote::choice,
      diesel::dsl::count_star(),
    ))
    .load(conn)
    .await?;
  let mine: HashMap<i32, i32> = post_poll_vote::table
    .filter(post_poll_vote::post_id.eq_any(ids))
    .filter(post_poll_vote::person_id.eq(me.unwrap_or(-1))) // guests: no votes
    .select((post_poll_vote::post_id, post_poll_vote::choice))
    .load::<(i32, i32)>(conn)
    .await?
    .into_iter()
    .collect();
  Ok(
    polls
      .into_iter()
      .map(|(post_id, question, options, ends_at)| {
        let my_vote = mine.get(&post_id).copied();
        let show = my_vote.is_some() || ends_at <= Utc::now() || creators.get(&post_id) == me.as_ref();
        let counts = show.then(|| {
          let mut n = vec![0; options.len()];
          for (_, choice, count) in counts.iter().filter(|c| c.0 == post_id) {
            if let Some(slot) = usize::try_from(*choice).ok().and_then(|i| n.get_mut(i)) {
              *slot = *count;
            }
          }
          n
        });
        PostPollView {
          post_id,
          question,
          options,
          ends_at,
          my_vote,
          counts,
        }
      })
      .collect(),
  )
}

async fn load_one(
  conn: &mut AsyncPgConnection,
  post_id: i32,
  me: i32,
) -> LemmyResult<Json<PostPollView>> {
  Ok(Json(
    load(conn, &[post_id], Some(me))
      .await?
      .pop()
      .ok_or(LemmyErrorType::NotFound)?,
  ))
}

#[derive(Debug, Deserialize)]
pub struct CreatePostPoll {
  pub post_id: i32,
  pub question: String,
  pub options: Vec<String>,
  pub ends_at: DateTime<Utc>,
}

/// PUT /post_poll: the post's creator adds a poll to their creator page post (once).
pub async fn create_post_poll(
  Json(data): Json<CreatePostPoll>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostPollView>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let me = local_user_view.person.id.0;
  let question = data.question.trim().to_string();
  let options: Vec<String> = data
    .options
    .iter()
    .map(|o| o.trim().to_string())
    .filter(|o| !o.is_empty())
    .collect();
  if question.is_empty()
    || question.chars().count() > MAX_QUESTION_LENGTH
    || !(2..=MAX_OPTIONS).contains(&options.len())
    || options.iter().any(|o| o.chars().count() > MAX_OPTION_LENGTH)
    || data.ends_at <= Utc::now()
  {
    return Err(LemmyErrorType::Unknown("invalid_poll".into()).into());
  }
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let (creator_id, community_name): (i32, String) = post::table
    .inner_join(community::table)
    .filter(post::id.eq(data.post_id))
    .select((post::creator_id, community::name))
    .first(conn)
    .await
    .optional()?
    .ok_or(LemmyErrorType::NotFound)?;
  if creator_id != me || community_name != "creators" {
    return Err(LemmyErrorType::Unknown("polls_only_on_your_creator_page".into()).into());
  }
  let added = insert_into(post_poll::table)
    .values((
      post_poll::post_id.eq(data.post_id),
      post_poll::question.eq(question),
      post_poll::options.eq(options),
      post_poll::ends_at.eq(data.ends_at),
    ))
    .on_conflict_do_nothing()
    .execute(conn)
    .await?;
  if added == 0 {
    return Err(LemmyErrorType::Unknown("post_already_has_poll".into()).into());
  }
  load_one(conn, data.post_id, me).await
}

#[derive(Debug, Deserialize)]
pub struct GetPostPolls {
  /// Comma-separated post ids.
  pub post_ids: String,
}

#[derive(Debug, Serialize)]
pub struct PostPollsResponse {
  pub polls: Vec<PostPollView>,
}

/// GET /post_polls?post_ids=1,2,3: the polls among these posts.
pub async fn get_post_polls(
  Query(data): Query<GetPostPolls>,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<PostPollsResponse>> {
  let ids: Vec<i32> = data
    .post_ids
    .split(',')
    .filter_map(|id| id.trim().parse().ok())
    .take(MAX_POST_IDS)
    .collect();
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let polls = load(conn, &ids, local_user_view.map(|u| u.person.id.0)).await?;
  Ok(Json(PostPollsResponse { polls }))
}

#[derive(Debug, Deserialize)]
pub struct VotePostPoll {
  pub post_id: i32,
  pub choice: i32,
}

/// POST /post_poll/vote: one vote per person, before the poll ends. It can't be changed.
pub async fn vote_post_poll(
  Json(data): Json<VotePostPoll>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostPollView>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let me = local_user_view.person.id.0;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  // The post must still be visible.
  post::table
    .find(data.post_id)
    .filter(post::deleted.eq(false))
    .filter(post::removed.eq(false))
    .select(post::id)
    .first::<i32>(conn)
    .await
    .optional()?
    .ok_or(LemmyErrorType::NotFound)?;
  let (options, ends_at): (Vec<String>, DateTime<Utc>) = post_poll::table
    .find(data.post_id)
    .select((post_poll::options, post_poll::ends_at))
    .first(conn)
    .await
    .optional()?
    .ok_or(LemmyErrorType::NotFound)?;
  if ends_at <= Utc::now() {
    return Err(LemmyErrorType::Unknown("poll_ended".into()).into());
  }
  if usize::try_from(data.choice).map_or(true, |c| c >= options.len()) {
    return Err(LemmyErrorType::Unknown("invalid_poll_choice".into()).into());
  }
  let added = insert_into(post_poll_vote::table)
    .values((
      post_poll_vote::post_id.eq(data.post_id),
      post_poll_vote::person_id.eq(me),
      post_poll_vote::choice.eq(data.choice),
    ))
    .on_conflict_do_nothing()
    .execute(conn)
    .await?;
  if added == 0 {
    return Err(LemmyErrorType::Unknown("already_voted".into()).into());
  }
  load_one(conn, data.post_id, me).await
}
