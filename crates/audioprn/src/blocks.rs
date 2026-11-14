//! Blocking works both ways, like Reddit (see the audioprn_two_way_blocks migration): you don't
//! see people you blocked, and people who blocked you don't exist for you. Admins and moderators
//! are never hidden from. Each user sees who they blocked and who blocked them, to block back.

use activitypub_federation::config::Data;
use actix_web::web::Json;
use diesel::{
  ExpressionMethods,
  QueryDsl,
  dsl::{exists, select},
  sql_query,
  sql_types::{Bool, Integer, Nullable, Text},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::context::LemmyContext;
use lemmy_db_schema_file::{PersonId, schema::person_actions};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::{DbPool, get_conn};
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use serde::Serialize;

/// Fails with NotFound when `other` blocked the logged-in user.
pub async fn check_not_blocked_by(
  pool: &mut DbPool<'_>,
  me: Option<&LocalUserView>,
  other: PersonId,
) -> LemmyResult<()> {
  let Some(me) = me else { return Ok(()) };
  let conn = &mut get_conn(pool).await?;
  let blocked: bool = select(exists(
    person_actions::table
      .find((me.person.id, other))
      .filter(person_actions::blocked_by_at.is_not_null()),
  ))
  .get_result(conn)
  .await?;
  if blocked {
    Err(LemmyErrorType::NotFound.into())
  } else {
    Ok(())
  }
}

#[derive(Debug, Serialize, diesel::QueryableByName)]
pub struct BlockedPerson {
  #[diesel(sql_type = Integer)]
  pub id: i32,
  #[diesel(sql_type = Text)]
  pub name: String,
  #[diesel(sql_type = Nullable<Text>)]
  pub display_name: Option<String>,
  #[diesel(sql_type = Bool)]
  pub blocked_by_me: bool,
  #[diesel(sql_type = Bool)]
  pub blocked_me: bool,
}

#[derive(Debug, Serialize)]
pub struct BlocksResponse {
  pub people: Vec<BlockedPerson>,
}

/// GET /blocks: everyone the logged-in user blocked or was blocked by. Only they see this.
pub async fn get_blocks(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<BlocksResponse>> {
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let people = sql_query(
    "SELECT p.id, p.name, p.display_name,
       EXISTS (SELECT 1 FROM person_actions WHERE person_id = $1 AND target_id = p.id
         AND blocked_at IS NOT NULL) AS blocked_by_me,
       EXISTS (SELECT 1 FROM person_actions WHERE person_id = p.id AND target_id = $1
         AND blocked_at IS NOT NULL) AS blocked_me
     FROM person p
     WHERE NOT p.deleted AND p.id IN (
       SELECT target_id FROM person_actions WHERE person_id = $1 AND blocked_at IS NOT NULL
       UNION SELECT person_id FROM person_actions WHERE target_id = $1 AND blocked_at IS NOT NULL)
     ORDER BY lower(coalesce(p.display_name, p.name))",
  )
  .bind::<Integer, _>(local_user_view.person.id.0)
  .load(conn)
  .await?;
  Ok(Json(BlocksResponse { people }))
}
