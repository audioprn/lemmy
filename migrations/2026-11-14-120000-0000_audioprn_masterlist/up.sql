-- audioprn: creators' masterlists. Each creator has their own categories: an icon, the tags that
-- put an audio in it (aliases, `*` = any text) and categories added along with it. Masterlist
-- entries are audios elsewhere (free or paywalled); the creator's own posts are listed too.
CREATE TABLE masterlist_category (
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    position int NOT NULL,
    name text NOT NULL,
    icon text NOT NULL,
    aliases text[] NOT NULL,
    also text[] NOT NULL,
    PRIMARY KEY (person_id, position)
);

CREATE TABLE masterlist_entry (
    id serial PRIMARY KEY,
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    title text NOT NULL,
    description text NOT NULL,
    date date,
    duration text NOT NULL,
    writer text NOT NULL,
    writer_link text,
    credits text NOT NULL,
    tags text[] NOT NULL,
    -- None: worked out from the tags; otherwise the categories the creator chose.
    categories text[],
    links text[] NOT NULL,
    script_link text,
    paywalled boolean NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_masterlist_entry_person ON masterlist_entry (person_id);

