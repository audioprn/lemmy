//! audioprn awards. Anyone who posted or commented in the past 7 days has one award to give
//! (renewed 7 days after they last gave one). Verified creators also get 10 awards each month
//! after a month with Content page posts. Unused awards don't carry over. One award per person per
//! post (they pick the type: Goat, Kino, Dropped Crown, Uwu or Respect), not on their own posts,
//! and it can't be taken back. Awards don't change Engagement. Posts imported from Reddit don't
//! count as posting (they can carry any date).

use crate::reaction::{PostReactions, load};
use activitypub_federation::config::Data;
use actix_web::web::Json;
use diesel::{
  QueryableByName,
  sql_query,
  sql_types::{BigInt, Bool, Integer},
};
use diesel_async::{AsyncConnection, RunQueryDsl};
use lemmy_api_utils::{context::LemmyContext, utils::check_local_user_banned_or_deleted};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyError, LemmyErrorType, LemmyResult};
use serde::{Deserialize, Serialize};

diesel::table! {
  post_award (post_id, giver_id) {
    post_id -> Int4,
    giver_id -> Int4,
    pool -> Text,
    kind -> Text,
  }
}

/// Goat, Kino, Dropped Crown, Uwu, Respect.
const KINDS: [&str; 5] = ["goat", "kino", "dropped_crown", "uwu", "respect"];

/// The awards a person can give now.
#[derive(Debug, Serialize, QueryableByName)]
pub struct Awards {
  /// The weekly award is available.
  #[diesel(sql_type = Bool)]
  pub weekly: bool,
  /// Creator awards left this month.
  #[diesel(sql_type = BigInt)]
  pub monthly: i64,
}

const AWARDS_SQL: &str = "SELECT
  (EXISTS (SELECT 1 FROM post WHERE creator_id = $1 AND NOT deleted
      AND published_at > now() - interval '7 days'
      AND id NOT IN (SELECT post_id FROM reddit_import))
    OR EXISTS (SELECT 1 FROM comment WHERE creator_id = $1 AND NOT deleted
      AND published_at > now() - interval '7 days'))
  AND NOT EXISTS (SELECT 1 FROM post_award WHERE giver_id = $1 AND pool = 'weekly'
      AND published_at > now() - interval '7 days') AS weekly,
  CASE WHEN EXISTS (SELECT 1 FROM creator_application WHERE person_id = $1 AND approved)
    AND EXISTS (SELECT 1 FROM post p JOIN community c ON c.id = p.community_id
      WHERE p.creator_id = $1 AND NOT p.deleted AND NOT p.removed AND c.name <> 'creators'
        AND NOT EXISTS (SELECT 1 FROM post_off_content o WHERE o.post_id = p.id)
        AND p.id NOT IN (SELECT post_id FROM reddit_import)
        AND p.published_at >= date_trunc('month', now()) - interval '1 month'
        AND p.published_at < date_trunc('month', now()))
  THEN 10 - (SELECT count(*) FROM post_award WHERE giver_id = $1 AND pool = 'monthly'
      AND published_at >= date_trunc('month', now()))
  ELSE 0 END AS monthly";

async fn my_awards(context: &LemmyContext, person_id: i32) -> LemmyResult<Awards> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  Ok(
    sql_query(AWARDS_SQL)
      .bind::<Integer, _>(person_id)
      .get_result(conn)
      .await?,
  )
}

/// GET /awards: the logged-in user's awards to give.
pub async fn get_awards(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<Awards>> {
  Ok(Json(my_awards(&context, local_user_view.person.id.0).await?))
}

#[derive(Debug, Deserialize)]
pub struct GivePostAward {
  pub post_id: i32,
  pub kind: String,
}

/// PUT /post_award: give an award to a post (the weekly one first). Returns its reactions.
pub async fn give_post_award(
  Json(data): Json<GivePostAward>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostReactions>> {
  check_local_user_banned_or_deleted(&local_user_view)?;
  if !KINDS.contains(&data.kind.as_str()) {
    return Err(LemmyErrorType::Unknown("invalid_award".into()).into());
  }
  let me = local_user_view.person.id.0;
  {
    let mut pool = context.pool();
    let conn = &mut get_conn(&mut pool).await?;
    conn
      .transaction::<_, LemmyError, _>(async |conn| {
        // One at a time per giver, so parallel requests can't spend the same award twice.
        sql_query("SELECT pg_advisory_xact_lock(hashtext('post_award'), $1)")
          .bind::<Integer, _>(me)
          .execute(conn)
          .await?;
        let awards: Awards = sql_query(AWARDS_SQL)
          .bind::<Integer, _>(me)
          .get_result(conn)
          .await?;
        let award_pool = if awards.weekly {
          "weekly"
        } else if awards.monthly > 0 {
          "monthly"
        } else {
          return Err(LemmyErrorType::Unknown("no_awards_left".into()).into());
        };
        // Only visible posts by someone else; once per person per post.
        let added = sql_query(
          "INSERT INTO post_award (post_id, giver_id, pool, kind)
           SELECT id, $2, $3, $4 FROM post
           WHERE id = $1 AND creator_id <> $2 AND NOT deleted AND NOT removed
           ON CONFLICT DO NOTHING",
        )
        .bind::<Integer, _>(data.post_id)
        .bind::<Integer, _>(me)
        .bind::<diesel::sql_types::Text, _>(award_pool)
        .bind::<diesel::sql_types::Text, _>(&data.kind)
        .execute(conn)
        .await?;
        if added == 0 {
          return Err(LemmyErrorType::Unknown("cant_award_post".into()).into());
        }
        Ok(())
      })
      .await?;
  }
  let reactions = load(&context, &[data.post_id], Some(me))
    .await?
    .pop()
    .ok_or(LemmyErrorType::NotFound)?;
  Ok(Json(reactions))
}
