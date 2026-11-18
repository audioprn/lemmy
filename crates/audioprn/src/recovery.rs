//! audioprn: no email (user, 2026-11-18), so no password reset emails. Everyone keeps a recovery
//! code, shown once (after signing up, or made anew in Settings), to set a new password
//! themselves. A creator who lost the code too asks an admin, who sets a temporary password for
//! them (the backup, worst case). Admins don't use codes (an admin account is worth the most to
//! take over); their passwords are fixed on the server.

use crate::is_verified_creator;
use activitypub_federation::config::Data;
use actix_web::web::Json;
use bcrypt::{DEFAULT_COST, hash, verify};
use diesel::{
  QueryableByName,
  sql_query,
  sql_types::{Bool, Integer, Nullable, Text},
};
use diesel_async::RunQueryDsl;
use lemmy_api_utils::{
  context::LemmyContext,
  utils::{is_admin, password_length_check},
};
use lemmy_db_schema::source::{local_user::LocalUser, login_token::LoginToken};
use lemmy_db_schema_file::{PersonId, newtypes::LocalUserId};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_diesel_utils::connection::get_conn;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};
use rand::{RngExt, distr::Alphanumeric};
use serde::{Deserialize, Serialize};

/// Letters and digits that can't be mixed up (no 0/O, 1/I/L, U).
const CODE_CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";

fn error(name: &str) -> LemmyErrorType {
  LemmyErrorType::Unknown(name.into())
}

/// A new code: 16 characters (about 78 bits) in groups of four, "ABCD-EFGH-JKMN-PQRS".
fn new_code() -> String {
  let mut rng = rand::rng();
  let chars: Vec<char> = (0..16)
    .map(|_| char::from(CODE_CHARS[rng.random_range(0..CODE_CHARS.len())]))
    .collect();
  chars
    .chunks(4)
    .map(|c| c.iter().collect::<String>())
    .collect::<Vec<_>>()
    .join("-")
}

/// What was typed, as stored: capitals, no dashes or spaces.
fn normalise(code: &str) -> String {
  code
    .chars()
    .filter(char::is_ascii_alphanumeric)
    .map(|c| c.to_ascii_uppercase())
    .collect()
}

/// Makes the user's recovery code (replacing any old one) and returns it, the only time it's shown.
async fn make_code(context: &LemmyContext, local_user_id: LocalUserId) -> LemmyResult<String> {
  let code = new_code();
  let code_hash = hash(normalise(&code), DEFAULT_COST)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  sql_query(
    "INSERT INTO recovery_code (local_user_id, code_hash) VALUES ($1, $2)
     ON CONFLICT (local_user_id) DO UPDATE SET code_hash = $2, created_at = now()",
  )
  .bind::<Integer, _>(local_user_id.0)
  .bind::<Text, _>(code_hash)
  .execute(conn)
  .await?;
  Ok(code)
}

#[derive(Serialize)]
pub struct RecoveryCode {
  pub code: String,
}

/// POST /recovery_code: a new recovery code for the logged-in user (the old one stops working).
pub async fn create_recovery_code(
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<RecoveryCode>> {
  let user = &local_user_view.local_user;
  if user.admin {
    return Err(error("admins_dont_use_codes").into());
  }
  let code = make_code(&context, user.id).await?;
  Ok(Json(RecoveryCode { code }))
}

#[derive(Deserialize)]
pub struct Recover {
  pub username: String,
  pub code: String,
  pub new_password: String,
}

#[derive(QueryableByName)]
struct Account {
  #[diesel(sql_type = Integer)]
  local_user_id: i32,
  #[diesel(sql_type = Bool)]
  admin: bool,
  #[diesel(sql_type = Nullable<Text>)]
  code_hash: Option<String>,
}

/// POST /recover (rate limited like signups): a user sets a new password with their username
/// and recovery code. Every login ends, the code is used up, and a new one is returned.
pub async fn recover(
  Json(data): Json<Recover>,
  context: Data<LemmyContext>,
) -> LemmyResult<Json<RecoveryCode>> {
  password_length_check(&data.new_password)?;
  let mut pool = context.pool();
  let conn = &mut get_conn(&mut pool).await?;
  let account: Option<Account> = sql_query(
    "SELECT lu.id AS local_user_id, lu.admin, rc.code_hash
     FROM person p JOIN local_user lu ON lu.person_id = p.id
     LEFT JOIN recovery_code rc ON rc.local_user_id = lu.id
     WHERE lower(p.name) = lower($1) AND p.local AND NOT p.deleted",
  )
  .bind::<Text, _>(data.username.trim().trim_start_matches("u/"))
  .get_result(conn)
  .await
  .ok();
  // The same answer for an unknown name, an admin, a wrong code or no code.
  let invalid = || error("invalid_recovery_code");
  let account = account.filter(|a| !a.admin).ok_or_else(invalid)?;
  let code_hash = account.code_hash.ok_or_else(invalid)?;
  if !verify(normalise(&data.code), &code_hash).unwrap_or(false) {
    return Err(invalid().into());
  }
  let local_user_id = LocalUserId(account.local_user_id);
  LocalUser::update_password(&mut context.pool(), local_user_id, &data.new_password).await?;
  LoginToken::invalidate_all(&mut context.pool(), local_user_id).await?;
  let code = make_code(&context, local_user_id).await?;
  Ok(Json(RecoveryCode { code }))
}

#[derive(Deserialize)]
pub struct AdminResetPassword {
  pub person_id: i32,
}

#[derive(Serialize)]
pub struct TemporaryPassword {
  pub password: String,
}

/// POST /admin_reset_password (admins): sets a temporary password for a creator who lost theirs
/// (and their code), shown once to the admin to pass on. Their logins end. Not for admins.
pub async fn admin_reset_password(
  Json(data): Json<AdminResetPassword>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<TemporaryPassword>> {
  is_admin(&local_user_view)?;
  let person_id = PersonId(data.person_id);
  let target = LocalUserView::read_person(&mut context.pool(), person_id).await?;
  if target.local_user.admin {
    return Err(error("cant_reset_an_admin").into());
  }
  // The backup is for creators only; listeners rely on their code.
  if !is_verified_creator(&mut context.pool(), person_id).await? {
    return Err(error("only_creators_get_a_reset").into());
  }
  let password: String = rand::rng()
    .sample_iter(Alphanumeric)
    .take(16)
    .map(char::from)
    .collect();
  let local_user_id = target.local_user.id;
  LocalUser::update_password(&mut context.pool(), local_user_id, &password).await?;
  LoginToken::invalidate_all(&mut context.pool(), local_user_id).await?;
  Ok(Json(TemporaryPassword { password }))
}
