//! audioprn: import one post from a creator's Reddit data export. The export is read in their
//! browser, which sends only the posts they pick. Each goes through `create_post`, so every normal
//! check applies; then it keeps its Reddit date, gets its credits, and its Reddit id is remembered
//! so it's only imported once (see `lemmy_audioprn::reddit`).

use super::create::create_post;
use activitypub_federation::config::Data;
use actix_web::web::Json;
use chrono::{DateTime, Utc};
use lemmy_api_utils::context::LemmyContext;
use lemmy_audioprn::post::{PostCredit, SetPostCredits, set_post_credits};
use lemmy_db_views_local_user::LocalUserView;
use lemmy_db_views_post::api::{CreatePost, PostResponse};
use lemmy_utils::error::LemmyResult;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ImportRedditPost {
  pub reddit_id: String,
  /// The same post's cross-posts in other subreddits.
  #[serde(default)]
  pub also_reddit_ids: Vec<String>,
  /// When it was posted on Reddit.
  pub date: DateTime<Utc>,
  pub credits: Vec<PostCredit>,
  pub post: CreatePost,
}

/// POST /audioprn/reddit_import
pub async fn import_reddit_post(
  Json(data): Json<ImportRedditPost>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<PostResponse>> {
  let me = local_user_view.person.id;
  lemmy_audioprn::reddit::check_import(
    &context,
    me,
    &data.reddit_id,
    &data.also_reddit_ids,
    data.date,
  )
  .await?;
  let res = create_post(Json(data.post), context.clone(), local_user_view.clone()).await?;
  let post_id = res.post_view.post.id;
  lemmy_audioprn::reddit::finish_import(
    &context,
    me,
    &data.reddit_id,
    &data.also_reddit_ids,
    post_id,
    data.date,
  )
  .await?;
  if !data.credits.is_empty() {
    set_post_credits(
      Json(SetPostCredits {
        post_id,
        credits: data.credits,
        off_content: None,
      }),
      context,
      local_user_view,
    )
    .await?;
  }
  Ok(res)
}
