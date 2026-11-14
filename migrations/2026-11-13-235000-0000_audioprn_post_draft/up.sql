-- audioprn: post drafts. The website saves the whole post form as text (JSON) with its title;
-- only the writer sees their drafts. (IF NOT EXISTS: this once shared a version number with
-- audioprn_tags_skip_links, so some databases already have the table.)
CREATE TABLE IF NOT EXISTS post_draft (
    id serial PRIMARY KEY,
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    title text NOT NULL,
    data text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_post_draft_person ON post_draft (person_id, updated_at DESC);

