//! audioprn: content filters for the post list. Tags are read from posts into `post_filter_tags`
//! and `taboo_tag` by a database trigger (see the audioprn_post_filter_tags migration).

/// Builds a SQL condition from comma-separated filter names. Only fixed SQL is used, never the
/// input text, so this is safe to pass to `sql()`. Unknown names are ignored.
///
/// - `f_listener`, `m_listener`: content for these listeners (posts without a listener tag always
///   show). Leaving both out shows only posts without a listener tag.
/// - `f_voices`, `m_voices`: only posts with these voices.
/// - `no_taboo`, `no_rape`, `no_incest`: hide posts with these tags.
/// - `scripts`, `audios`: only posts with a script / audio flair.
/// - `followed`: only posts by creators the user follows (ignored for guests).
/// - `content_page`: the Content page: no posts kept off it, no creators' page posts, and no
///   posts tagged [SFW] unless `sfw` is on (the Content page is NSFW only by default).
/// - `my_recent`, `my_feed`: the two parts of the user's My audioprn feed.
/// - `saved`: only the user's saved posts.
pub fn content_filter_sql(filters: &str, my_person_id: Option<i32>) -> String {
  let on: Vec<&str> = filters.split(',').map(str::trim).collect();
  let has = |name: &str| on.contains(&name);
  let tags = |cond: &str| {
    format!("EXISTS (SELECT 1 FROM post_filter_tags t WHERE t.post_id = post.id AND {cond})")
  };
  let mut conds = vec![format!(
    "NOT {}",
    tags(&format!(
      "(t.listener_f OR t.listener_m) AND NOT (({} AND t.listener_f) OR ({} AND t.listener_m))",
      has("f_listener"),
      has("m_listener")
    ))
  )];
  for (name, cond) in [("f_voices", "t.voice_f"), ("m_voices", "t.voice_m")] {
    if has(name) {
      conds.push(tags(cond));
    }
  }
  for (name, cond) in [
    ("no_taboo", "t.taboo"),
    ("no_rape", "t.rape"),
    ("no_incest", "t.incest"),
  ] {
    if has(name) {
      conds.push(format!("NOT {}", tags(cond)));
    }
  }
  let mut flairs = vec![];
  if has("scripts") {
    flairs.push("'script_offer'");
  }
  if has("audios") {
    flairs.extend(["'script_fill'", "'oc'"]);
  }
  if !flairs.is_empty() {
    conds.push(format!(
      "EXISTS (SELECT 1 FROM post_community_tag pct JOIN community_tag ct ON ct.id = \
       pct.community_tag_id WHERE pct.post_id = post.id AND NOT ct.deleted AND ct.name IN ({}))",
      flairs.join(", ")
    ));
  }
  if has("content_page") {
    conds.push(
      "NOT EXISTS (SELECT 1 FROM post_off_content o WHERE o.post_id = post.id) AND NOT EXISTS \
       (SELECT 1 FROM community c WHERE c.id = post.community_id AND c.name = 'creators')"
        .into(),
    );
    if !has("sfw") {
      conds.push(format!("NOT {}", tags("'SFW' = ANY (t.tags)")));
    }
  }
  // My audioprn (see the audioprn_my_feed migration): `my_recent` = followed creators' posts from
  // the past 7 days, `my_feed` = everything else in the user's feed.
  for (name, part) in [("my_recent", "recent"), ("my_feed", "feed")] {
    if has(name) {
      conds.push(match my_person_id {
        Some(id) => format!("audioprn_my_feed(post.id, {id}) = '{part}'"),
        None => "FALSE".into(),
      });
    }
  }
  if has("saved") {
    conds.push(match my_person_id {
      Some(id) => format!(
        "EXISTS (SELECT 1 FROM post_actions pa WHERE pa.post_id = post.id AND pa.person_id = \
         {id} AND pa.saved_at IS NOT NULL)"
      ),
      None => "FALSE".into(),
    });
  }
  // Ignored for guests (a filter remembered in the browser from before logging out).
  if let (true, Some(id)) = (has("followed"), my_person_id) {
    conds.push(format!(
      "post.creator_id IN (SELECT target_id FROM person_actions WHERE person_id = {id} AND \
       followed_at IS NOT NULL)"
    ));
  }
  conds.join(" AND ")
}
