-- audioprn: post drafts. The website saves the whole post form as text (JSON) with its title;
-- only the writer sees their drafts.
CREATE TABLE post_draft (
    id serial PRIMARY KEY,
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    title text NOT NULL,
    data text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_post_draft_person ON post_draft (person_id, updated_at DESC);

