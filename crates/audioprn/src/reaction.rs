//! audioprn emoji reactions on posts: one of five emoji per user per post, instead of votes.
//! The post's engagement score is kept up to date by a database trigger
//! (see the audioprn_post_reaction migration). Your own favourites (♥) come with them.

use activitypub_federation::config::Data;
use actix_web::web::{Json, Query};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, dsl::insert_into};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_schema_file::{newtypes::PostId, schema::post};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

diesel::table! {
  post_reaction (post_id, person_id) {
    post_id -> Int4,
    person_id -> Int4,
    emoji -> Text,
  }
}

diesel::table! {
  post_favourite (person_id, post_id) {
    person_id -> Int4,
    post_id -> Int4,
  }
}

const EMOJI: [&str; 5] = ["fire", "gem", "black_heart", "pleading", "droplets"];
const MAX_POST_IDS: usize = 100;

#[derive(Debug, Default, Serialize)]
pub struct PostReactions {
  pub post_id: i32,
  /// Count per emoji name.
  pub counts: HashMap<String, i64>,
  /// The logged-in user's reaction.
  pub mine: Option<String>,
  /// Awards given to the post (count per type), and whether the logged-in user gave one.
  pub awards: HashMap<String, i64>,
  pub awarded: bool,
  /// The logged-in user marked it a favourite.
  pub favourite: bool,
}

#[derive(Debug, Serialize)]
pub struct PostReactionsResponse {
  pub reactions: Vec<PostReactions>,
}

pub(crate) async fn load(
  context: &LemmyContext,
  ids: &[i32],
  person_id: Option<i32>,
) -> LemmyResult<Vec<PostReactions>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let rows: Vec<(i32, i32, String)> = post_reaction::table
    .filter(post_reaction::post_id.eq_any(ids))
    .select((
      post_reaction::post_id,
      post_reaction::person_id,
      post_reaction::emoji,
    ))
    .load(conn)
    .await?;
  let mut by_post: HashMap<i32, PostReactions> = ids
    .iter()
    .map(|&post_id| {
      (
        post_id,
        PostReactions {
          post_id,
          ..Default::default()
        },
      )
    })
    .collect();
  for (post_id, reactor, emoji) in rows {
    if let Some(r) = by_post.get_mut(&post_id) {
      if Some(reactor) == person_id {
        r.mine = Some(emoji.clone());
      }
      *r.counts.entry(emoji).or_default() += 1;
    }
  }
  // Awards (see awards.rs) are shown with the reactions.
  let awards: Vec<(i32, i32, String)> = crate::awards::post_award::table
    .filter(crate::awards::post_award::post_id.eq_any(ids))
    .select((
      crate::awards::post_award::post_id,
      crate::awards::post_award::giver_id,
      crate::awards::post_award::kind,
    ))
    .load(conn)
    .await?;
  for (post_id, giver, kind) in awards {
    if let Some(r) = by_post.get_mut(&post_id) {
      *r.awards.entry(kind).or_default() += 1;
      r.awarded |= Some(giver) == person_id;
    }
  }
  if let Some(person_id) = person_id {
    let favourites: Vec<i32> = post_favourite::table
      .filter(post_favourite::person_id.eq(person_id))
      .filter(post_favourite::post_id.eq_any(ids))
      .select(post_favourite::post_id)
      .load(conn)
      .await?;
    for post_id in favourites {
      if let Some(r) = by_post.get_mut(&post_id) {
        r.favourite = true;
      }
    }
  }
  Ok(by_post.into_values().collect())
}

#[derive(Debug, Deserialize)]
pub struct GetPostReactions {
  /// Comma-separated post ids.
  pub post_ids: String,
}

/// GET /post_reactions?post_ids=1,2,3: counts for a page of posts, plus your own reaction.
pub async fn get_post_reactions(
  Query(data): Query<GetPostReactions>,
  context: Data<LemmyContext>,
  local_user_view: Option<LocalUserView>,
) -> LemmyResult<Json<PostReactionsResponse>> {
  let ids: Vec<i32> = data
    .post_ids
    .split(',')
    .filter_map(|id| id.trim().parse().ok())
    .take(MAX_POST_IDS)
    .collect();
  let person_id = local_user_view.map(|u| u.person.id.0);
  Ok(Json(PostReactionsResponse {
    reactions: load(&context, &ids, person_id).await?,
  }))
}

#[derive(Debug, Deserialize)]
pub struct SetPostReaction {
  pub post_id: PostId,
  /// One of the five emoji names, or none to remove your reaction.
  pub emoji: Option<String>,
}

/// PUT /post_reaction: set, change or remove your reaction to a post.
pub async fn set_post_reaction(
  Json(data): Json<SetPostReaction>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostReactions>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  let person_id = local_user_view.person.id.0;
  let post_id = data.post_id.0;
  {
    let mut pool = context.pool();
    let conn = &mut get_conn(&mut pool).await?;
    // The post must exist and be visible.
    post::table
      .find(post_id)
      .filter(post::deleted.eq(false))
      .filter(post::removed.eq(false))
      .select(post::id)
      .first::<i32>(conn)
      .await
      .optional()?
      .ok_or(LemmyErrorType::NotFound)?;

    match data.emoji {
      Some(emoji) if EMOJI.contains(&emoji.as_str()) => {
        insert_into(post_reaction::table)
          .values((
            post_reaction::post_id.eq(post_id),
            post_reaction::person_id.eq(person_id),
            post_reaction::emoji.eq(&emoji),
          ))
          .on_conflict((post_reaction::post_id, post_reaction::person_id))
          .do_update()
          .set(post_reaction::emoji.eq(&emoji))
          .execute(conn)
          .await?;
      }
      Some(_) => return Err(LemmyErrorType::Unknown("invalid_reaction".into()).into()),
      None => {
        diesel::delete(
          post_reaction::table
            .filter(post_reaction::post_id.eq(post_id))
            .filter(post_reaction::person_id.eq(person_id)),
        )
        .execute(conn)
        .await?;
      }
    }
  }
  let reactions = load(&context, &[post_id], Some(person_id))
    .await?
    .pop()
    .ok_or(LemmyErrorType::NotFound)?;
  Ok(Json(reactions))
}

#[derive(Debug, Deserialize)]
pub struct SetPostFavourite {
  pub post_id: PostId,
  pub favourite: bool,
}

/// PUT /post_favourite: mark a post a favourite (♥), or not.
pub async fn set_post_favourite(
  Json(data): Json<SetPostFavourite>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostReactions>> {
  let person_id = local_user_view.person.id.0;
  let post_id = data.post_id.0;
  {
    let mut pool = context.pool();
    let conn = &mut get_conn(&mut pool).await?;
    if data.favourite {
      // Only posts that exist and are visible.
      let added = diesel::sql_query(
        "INSERT INTO post_favourite (person_id, post_id)
         SELECT $1, id FROM post WHERE id = $2 AND NOT deleted AND NOT removed
         ON CONFLICT DO NOTHING",
      )
      .bind::<diesel::sql_types::Integer, _>(person_id)
      .bind::<diesel::sql_types::Integer, _>(post_id)
      .execute(conn)
      .await?;
      let exists = added > 0
        || post_favourite::table
          .find((person_id, post_id))
          .select(post_favourite::post_id)
          .first::<i32>(conn)
          .await
          .optional()?
          .is_some();
      if !exists {
        return Err(LemmyErrorType::NotFound.into());
      }
    } else {
      diesel::delete(post_favourite::table.find((person_id, post_id)))
        .execute(conn)
        .await?;
    }
  }
  let reactions = load(&context, &[post_id], Some(person_id))
    .await?
    .pop()
    .ok_or(LemmyErrorType::NotFound)?;
  Ok(Json(reactions))
}
