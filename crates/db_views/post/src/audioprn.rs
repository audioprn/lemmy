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
/// - `favourites`: only the user's favourite posts (♥).
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
  if has("favourites") {
    conds.push(match my_person_id {
      Some(id) => format!(
        "EXISTS (SELECT 1 FROM post_favourite pf WHERE pf.post_id = post.id AND pf.person_id = \
         {id})"
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

/// A search in gwasi's syntax (https://gwasi.com, Help), turned into a SQL condition on `post`.
/// The user's text only ever goes into the returned parameters: the condition refers to them as
/// `s.p[1]`, `s.p[2]`… and is wrapped as `(SELECT cond FROM (SELECT $1 AS p) s)` with the
/// parameters bound as one text array.
///
/// - `word`, `"exact phrase"`, `/regex/`: in the title, text or credits.
/// - `[tag]`: posts with that bracket tag.
/// - `a b` = both, `a OR b`, `NOT a` = `-a`, `( )` to group.
/// - `title:`, `u:` (creator), `w:` (writer credit), `c:` (cummunity), `f:` (flair),
///   `type:`/`t:` audio/script/other (a/s/o), `date:14d` / `date:2020-11` / `date:A_B`.
/// - Numbers: `reactions:` (or `score:`), `comments:`, `fills:` (Script Offers' fills),
///   `userfills:` (all fills of the creator's scripts), `karma:` (reactions on all the creator's
///   posts), `length:` (minutes, or thousands of words for scripts), as `5`, `<5`, `>=5`, `5+`
///   or `3-10`.
///
/// Unknown `name:value` words are searched as text. Several searches (the user's and their saved
/// search) must all match. Errors are messages for the user.
pub fn search_sql(searches: &[&str]) -> Result<(String, Vec<String>), String> {
  let mut conds = vec![];
  let mut params = vec![];
  for q in searches {
    if q.chars().count() > 1000 {
      return Err("search_too_long".into());
    }
    let mut parser = Parser {
      toks: tokenize(q),
      i: 0,
      params,
      depth: 0,
    };
    if parser.toks.len() > 60 {
      return Err("search_too_long".into());
    }
    conds.push(parser.or()?);
    // Unmatched ")": search the rest too.
    while parser.i < parser.toks.len() {
      parser.i += 1;
      conds.push(parser.or()?);
    }
    params = parser.params;
  }
  Ok((conds.join(" AND "), params))
}

/// The user's saved search, when they use it (see the audioprn_search migration).
pub async fn saved_search(
  pool: &mut lemmy_diesel_utils::connection::DbPool<'_>,
  person_id: i32,
) -> lemmy_utils::error::LemmyResult<Option<String>> {
  use diesel::{OptionalExtension, QueryableByName, sql_query, sql_types};
  use diesel_async::RunQueryDsl;
  #[derive(QueryableByName)]
  struct Saved {
    #[diesel(sql_type = sql_types::Text)]
    saved_query: String,
  }
  let conn = &mut lemmy_diesel_utils::connection::get_conn(pool).await?;
  Ok(
    sql_query(
      "SELECT saved_query FROM search_prefs WHERE person_id = $1 AND use_saved AND \
       saved_query <> ''",
    )
    .bind::<sql_types::Integer, _>(person_id)
    .get_result::<Saved>(conn)
    .await
    .optional()?
    .map(|s| s.saved_query),
  )
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
  Open,
  Close,
  Or,
  And,
  Not,
  Text(String),
  Regex(String),
  Tag(String),
  Field(String, String),
}

const FIELDS: &[&str] = &[
  "title", "u", "user", "w", "writer", "c", "community", "cummunity", "f", "flair", "type", "t",
  "date", "reactions", "score", "comments", "fills", "userfills", "karma", "length",
];

fn tokenize(q: &str) -> Vec<Tok> {
  let chars: Vec<char> = q.chars().collect();
  let mut toks = vec![];
  let mut i = 0;
  // Reads up to `end` (or the end of the search), after the opening character.
  let until = |i: &mut usize, end: char| -> Option<String> {
    let start = *i + 1;
    let len = chars[start..].iter().position(|&c| c == end)?;
    *i = start + len + 1;
    Some(chars[start..start + len].iter().collect())
  };
  while i < chars.len() {
    let c = chars[i];
    if c.is_whitespace() {
      i += 1;
    } else if c == '(' {
      toks.push(Tok::Open);
      i += 1;
    } else if c == ')' {
      toks.push(Tok::Close);
      i += 1;
    } else if c == '-' && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) {
      toks.push(Tok::Not);
      i += 1;
    } else if let (true, Some(t)) = (c == '"', until(&mut i.clone(), '"')) {
      until(&mut i, '"');
      toks.push(Tok::Text(t));
    } else if let (true, Some(t)) = (c == '/', until(&mut i.clone(), '/')) {
      until(&mut i, '/');
      toks.push(Tok::Regex(t));
    } else if let (true, Some(t)) = (c == '[', until(&mut i.clone(), ']')) {
      until(&mut i, ']');
      toks.push(Tok::Tag(t));
    } else {
      let start = i;
      while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '(' && chars[i] != ')' {
        i += 1;
      }
      let word: String = chars[start..i].iter().collect();
      let field = word.split_once(':').filter(|(name, _)| {
        FIELDS.contains(&name.to_lowercase().as_str())
      });
      toks.push(match (word.as_str(), field) {
        ("OR", _) => Tok::Or,
        ("AND", _) => Tok::And,
        ("NOT", _) => Tok::Not,
        (_, Some((name, value))) => {
          // A quoted value: w:"Some Name".
          let value = if value.is_empty() && chars.get(i) == Some(&'"') {
            until(&mut i, '"').unwrap_or_default()
          } else {
            value.to_string()
          };
          Tok::Field(name.to_lowercase(), value)
        }
        _ => Tok::Text(word),
      });
    }
  }
  toks
}

struct Parser {
  toks: Vec<Tok>,
  i: usize,
  params: Vec<String>,
  depth: usize,
}

impl Parser {
  fn peek(&self) -> Option<&Tok> {
    self.toks.get(self.i)
  }

  fn param(&mut self, value: impl Into<String>) -> String {
    self.params.push(value.into());
    format!("s.p[{}]", self.params.len())
  }

  fn or(&mut self) -> Result<String, String> {
    let mut parts = vec![self.and()?];
    while self.peek() == Some(&Tok::Or) {
      self.i += 1;
      parts.push(self.and()?);
    }
    Ok(format!("({})", parts.join(" OR ")))
  }

  fn and(&mut self) -> Result<String, String> {
    let mut parts = vec![];
    loop {
      match self.peek() {
        None | Some(Tok::Close) | Some(Tok::Or) => break,
        Some(Tok::And) => self.i += 1,
        _ => parts.push(self.unary()?),
      }
    }
    Ok(if parts.is_empty() {
      "TRUE".into()
    } else {
      format!("({})", parts.join(" AND "))
    })
  }

  fn unary(&mut self) -> Result<String, String> {
    match self.peek() {
      Some(Tok::Not) => {
        self.i += 1;
        // `-` at the end: nothing to leave out.
        if matches!(self.peek(), None | Some(Tok::Close) | Some(Tok::Or)) {
          return Ok("TRUE".into());
        }
        Ok(format!("NOT coalesce({}, FALSE)", self.unary()?))
      }
      Some(Tok::Open) => {
        self.i += 1;
        self.depth += 1;
        if self.depth > 10 {
          return Err("search_too_long".into());
        }
        let inner = self.or()?;
        self.depth -= 1;
        if self.peek() == Some(&Tok::Close) {
          self.i += 1;
        }
        Ok(inner)
      }
      _ => {
        let tok = self.toks[self.i].clone();
        self.i += 1;
        self.term(tok)
      }
    }
  }

  fn term(&mut self, tok: Tok) -> Result<String, String> {
    Ok(match tok {
      Tok::Text(t) => {
        let p = self.param(t);
        format!(
          "(post.name ILIKE audioprn_like({p}) OR coalesce(post.body, '') ILIKE \
           audioprn_like({p}) OR EXISTS (SELECT 1 FROM post_credit c WHERE c.post_id = post.id \
           AND c.name ILIKE audioprn_like({p})))"
        )
      }
      Tok::Regex(r) => {
        let p = self.param(r);
        format!(
          "(audioprn_regex_match(post.name, {p}) OR audioprn_regex_match(coalesce(post.body, ''), \
           {p}))"
        )
      }
      Tok::Tag(t) => {
        let p = self.param(t.trim().to_uppercase());
        format!(
          "EXISTS (SELECT 1 FROM post_filter_tags t WHERE t.post_id = post.id AND {p} = ANY \
           (t.tags))"
        )
      }
      Tok::Field(name, value) => self.field(&name, &value)?,
      // Operators where a word should be: ignore them.
      Tok::Open | Tok::Close | Tok::Or | Tok::And | Tok::Not => "TRUE".into(),
    })
  }

  fn field(&mut self, name: &str, value: &str) -> Result<String, String> {
    let name_only = |v: &str| v.trim_start_matches("u/").to_string();
    let flair_in = |names: &str| {
      format!(
        "EXISTS (SELECT 1 FROM post_community_tag pct JOIN community_tag ct ON ct.id = \
         pct.community_tag_id WHERE pct.post_id = post.id AND NOT ct.deleted AND ct.name IN \
         ({names}))"
      )
    };
    let count = |what: &str| format!("(SELECT count(*) FROM {what})");
    Ok(match name {
      "title" => format!("post.name ILIKE audioprn_like({})", self.param(value)),
      "u" | "user" => format!(
        "post.creator_id IN (SELECT id FROM person WHERE lower(name) = lower({}))",
        self.param(name_only(value))
      ),
      "w" | "writer" => format!(
        "EXISTS (SELECT 1 FROM post_credit c WHERE c.post_id = post.id AND c.role = 'writer' AND \
         lower(regexp_replace(c.name, '^u/', '', 'i')) = lower({}))",
        self.param(name_only(value))
      ),
      "c" | "community" | "cummunity" => {
        let p = self.param(value);
        format!(
          "post.community_id IN (SELECT id FROM community WHERE lower(name) = lower({p}) OR \
           lower(title) = lower({p}))"
        )
      }
      "f" | "flair" => {
        let p = self.param(value);
        format!(
          "EXISTS (SELECT 1 FROM post_community_tag pct JOIN community_tag ct ON ct.id = \
           pct.community_tag_id WHERE pct.post_id = post.id AND NOT ct.deleted AND \
           (ct.display_name ILIKE audioprn_like({p}) OR ct.name ILIKE audioprn_like({p})))"
        )
      }
      "type" | "t" => match value.to_lowercase().as_str() {
        "a" | "audio" => flair_in("'script_fill', 'oc'"),
        "s" | "script" => flair_in("'script_offer'"),
        "o" | "other" => format!("NOT {}", flair_in("'script_fill', 'oc', 'script_offer'")),
        _ => return Err("search_bad_type".into()),
      },
      "date" => {
        let (from, to) = date_range(value).ok_or("search_bad_date")?;
        format!(
          "(post.published_at >= {}::timestamptz AND post.published_at < {}::timestamptz)",
          self.param(from.to_rfc3339()),
          self.param(to.to_rfc3339())
        )
      }
      _ => {
        let expr = match name {
          "reactions" | "score" => count("post_reaction r WHERE r.post_id = post.id"),
          "comments" => "post.comments".into(),
          "fills" => format!(
            "CASE WHEN audioprn_is_script_offer(post.id) THEN {} END",
            count("script_fill f WHERE f.script_post_id = post.id")
          ),
          "userfills" => count(
            "script_fill f JOIN post sp ON sp.id = f.script_post_id WHERE sp.creator_id = \
             post.creator_id",
          ),
          "karma" => count(
            "post_reaction r JOIN post kp ON kp.id = r.post_id WHERE kp.creator_id = \
             post.creator_id",
          ),
          _ => "audioprn_post_length(post.id)".into(),
        };
        let conds = number_range(value)
          .ok_or("search_bad_number")?
          .into_iter()
          .map(|(op, n)| format!("{expr} {op} {}::numeric", self.param(n.to_string())))
          .collect::<Vec<_>>();
        format!("({})", conds.join(" AND "))
      }
    })
  }
}

/// `5`, `<5`, `<=5`, `>5`, `>=5`, `5+` or `3-10`, as comparisons.
fn number_range(v: &str) -> Option<Vec<(&'static str, f64)>> {
  let num = |s: &str| s.trim().parse::<f64>().ok().filter(|n| n.is_finite());
  for (prefix, op) in [("<=", "<="), (">=", ">="), ("<", "<"), (">", ">"), ("=", "=")] {
    if let Some(rest) = v.strip_prefix(prefix) {
      return Some(vec![(op, num(rest)?)]);
    }
  }
  if let Some(rest) = v.strip_suffix('+') {
    return Some(vec![(">=", num(rest)?)]);
  }
  if let Some((a, b)) = v.split_once('-') {
    return Some(vec![(">=", num(a)?), ("<=", num(b)?)]);
  }
  Some(vec![("=", num(v)?)])
}

/// `14d` (the last 14 days; h/d/m/y = hours/days/months/years), `2020`, `2020-11`,
/// `2020-11-05`, or two of these joined by `_` for a range.
fn date_range(v: &str) -> Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> {
  use chrono::{Duration, Months, NaiveDate, Utc};
  let now = Utc::now();
  let far = now + Duration::days(1);
  // The start and end of a date written as year[-month[-day]].
  let period = |s: &str| -> Option<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let parts: Vec<u32> = s.split('-').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (start, end) = match parts[..] {
      [y] => {
        let d = NaiveDate::from_ymd_opt(y as i32, 1, 1)?;
        (d, d.checked_add_months(Months::new(12))?)
      }
      [y, m] => {
        let d = NaiveDate::from_ymd_opt(y as i32, m, 1)?;
        (d, d.checked_add_months(Months::new(1))?)
      }
      [y, m, d] => {
        let d = NaiveDate::from_ymd_opt(y as i32, m, d)?;
        (d, d.succ_opt()?)
      }
      _ => return None,
    };
    Some((
      start.and_hms_opt(0, 0, 0)?.and_utc(),
      end.and_hms_opt(0, 0, 0)?.and_utc(),
    ))
  };
  if let Some((a, b)) = v.split_once('_') {
    return Some((period(a)?.0, period(b)?.1));
  }
  let unit = v.chars().last()?;
  if "hdmy".contains(unit) {
    // `unit` is one byte, so this cuts at a character boundary.
    if let Ok(n) = v[..v.len() - 1].parse::<u32>() {
      let from = match unit {
        'h' => now.checked_sub_signed(Duration::hours(n.into()))?,
        'd' => now.checked_sub_signed(Duration::days(n.into()))?,
        'm' => now.checked_sub_months(Months::new(n))?,
        _ => now.checked_sub_months(Months::new(n.checked_mul(12)?))?,
      };
      return Some((from, far));
    }
  }
  period(v)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn search_syntax() {
    let (cond, params) = search_sql(&[r#"script offer -blood "slow burn" [F4M] w:u/Lurkydip"#]).unwrap();
    assert_eq!(params, ["script", "offer", "blood", "slow burn", "F4M", "Lurkydip"]);
    assert!(cond.contains("NOT coalesce("));
    let (cond, params) = search_sql(&["(fdom OR mdom) fills:0 length:10-20 t:a date:14d"]).unwrap();
    assert!(cond.contains(" OR "));
    assert_eq!(params[..2], ["fdom", "mdom"]);
    assert_eq!(params.len(), 2 + 1 + 2 + 2);
    // Unknown fields and URLs are searched as text.
    let (_, params) = search_sql(&["https://soundgasm.net/u/x foo:bar"]).unwrap();
    assert_eq!(params, ["https://soundgasm.net/u/x", "foo:bar"]);
    assert!(search_sql(&["t:video"]).is_err());
    assert!(search_sql(&["fills:many"]).is_err());
    assert!(search_sql(&["date:2020-2-10_2020-2-15"]).is_ok());
    assert_eq!(search_sql(&[""]).unwrap().0, "(TRUE)");
    // The user's text never reaches the SQL.
    let (cond, _) = search_sql(&["'; DROP TABLE post; -- \" ) ("]).unwrap();
    assert!(!cond.contains("DROP"));
  }
}
